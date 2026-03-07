/// Minimal tinyuz compressor for LED RGB frame data.
///
/// Produces output compatible with the tinyuz decompressor used by
/// Lian Li wireless fan firmware. Only implements the subset needed
/// for our highly-repetitive RGB data (literals + back-references).
///
/// Format reference: https://github.com/sisong/tinyuz

// Constants from tuz_types_private.h
const MIN_DICT_MATCH_LEN: usize = 2;
const MIN_LITERAL_LEN: usize = 15;
const MAX_TYPE_BIT_COUNT: usize = 8;
const BIG_POS_FOR_LEN: usize = (1 << 11) + (1 << 9) + (1 << 7) - 1;

// Control types (encoded as dict match with pos=0)
const CTRL_STREAM_END: usize = 3;

/// Bitstream writer that packs type bits into bytes (LSB-first).
struct TuzEncoder {
    code: Vec<u8>,
    types_index: usize,
    type_count: usize,
    dict_pos_back: usize,
    is_have_data_back: bool,
    is_need_literal_line: bool,
}

impl TuzEncoder {
    fn new(is_need_literal_line: bool) -> Self {
        Self {
            code: Vec::new(),
            types_index: 0,
            type_count: 0,
            dict_pos_back: 1,
            is_have_data_back: false,
            is_need_literal_line,
        }
    }

    fn out_dict_size(&mut self, dict_size: usize) {
        // Little-endian, 4 bytes (tuz_kDictSizeSavedBytes=4 when kMaxOfDictSize=(1<<30))
        self.code.push((dict_size & 0xFF) as u8);
        self.code.push(((dict_size >> 8) & 0xFF) as u8);
        self.code.push(((dict_size >> 16) & 0xFF) as u8);
        self.code.push(((dict_size >> 24) & 0xFF) as u8);
    }

    fn out_type(&mut self, bit: usize) {
        if self.type_count == 0 {
            self.types_index = self.code.len();
            self.code.push(0);
        }
        self.code[self.types_index] |= ((bit & 1) as u8) << self.type_count;
        self.type_count += 1;
        if self.type_count == MAX_TYPE_BIT_COUNT {
            self.type_count = 0;
        }
    }

    /// Encode a variable-length integer with given pack_bit width.
    /// Format: groups of (pack_bit data bits + 1 continuation bit), LSB first.
    /// Continuation bit 1 = more groups, 0 = last group.
    fn out_len(&mut self, v: usize, pack_bit: usize) {
        // Calculate how many groups needed and the adjusted value
        let mut groups = 1usize;
        let mut threshold = 1usize << pack_bit;
        let mut adjusted = v;

        while adjusted >= threshold {
            adjusted -= threshold;
            groups += 1;
            threshold <<= pack_bit;
        }

        // Output groups from most significant to least significant
        let remaining = adjusted;
        for g in (0..groups).rev() {
            for i in 0..pack_bit {
                self.out_type((remaining >> (g * pack_bit + i)) & 1);
            }
            // Continuation bit: 1 if more groups follow, 0 if last
            self.out_type(if g > 0 { 1 } else { 0 });
        }
    }

    fn out_dict_len(&mut self, len: usize) {
        self.out_len(len, 1); // kDictLenPackBit = 1
    }

    fn out_dict_pos_len(&mut self, len: usize) {
        self.out_len(len, 2); // kDictPosLenPackBit = 2
    }

    fn out_dict_pos(&mut self, pos: usize) {
        // pos is already saved_dict_pos (1-based, 0 reserved for ctrl)
        if pos >= (1 << 7) {
            let adjusted = pos - (1 << 7);
            self.code
                .push(((adjusted & ((1 << 7) - 1)) | (1 << 7)) as u8);
            self.out_dict_pos_len(adjusted >> 7);
        } else {
            self.code.push(pos as u8);
        }
    }

    /// Emit literal bytes.
    fn out_data(&mut self, data: &[u8]) {
        let len = data.len();
        if self.is_need_literal_line && len >= MIN_LITERAL_LEN {
            // Emit as literalLine control
            self.out_ctrl(1); // tuz_ctrlType_literalLine
            self.out_dict_pos_len(len - MIN_LITERAL_LEN);
            self.code.extend_from_slice(data);
        } else {
            for &b in data {
                self.out_type(1); // tuz_codeType_data
                self.code.push(b);
            }
        }
        self.is_have_data_back = true;
    }

    /// Emit a dictionary back-reference (match).
    fn out_dict(&mut self, match_len: usize, dict_pos: usize) {
        self.out_type(0); // tuz_codeType_dict
        let saved_dict_pos = dict_pos + 1; // 0 reserved for ctrl
        let is_same_pos = self.dict_pos_back == saved_dict_pos;
        let is_saved_same_pos = is_same_pos && self.is_have_data_back;

        let mut len = match_len - MIN_DICT_MATCH_LEN;
        if !is_saved_same_pos && saved_dict_pos > BIG_POS_FOR_LEN {
            len -= 1;
        }

        self.out_dict_len(len);
        if self.is_have_data_back {
            self.out_type(if is_saved_same_pos { 1 } else { 0 });
        }
        if !is_saved_same_pos {
            self.out_dict_pos(saved_dict_pos);
        }
        self.is_have_data_back = false;
        self.dict_pos_back = saved_dict_pos;
    }

    fn out_ctrl(&mut self, ctrl: usize) {
        self.out_type(0); // tuz_codeType_dict
        self.out_dict_len(ctrl);
        if self.is_have_data_back {
            self.out_type(0);
        }
        self.out_dict_pos(0); // pos=0 means control
    }

    fn out_stream_end(&mut self) {
        self.out_ctrl(CTRL_STREAM_END);
        // Reset state after control
        self.type_count = 0;
        self.dict_pos_back = 1;
        self.is_have_data_back = false;
    }
}

/// Compress data using tinyuz algorithm with greedy matching.
///
/// Uses a simple greedy approach: at each position, find the longest
/// back-reference within the dictionary window. For our highly repetitive
/// LED data this produces excellent compression.
pub fn tuz_compress(input: &[u8], dict_size: usize) -> Vec<u8> {
    let mut enc = TuzEncoder::new(true);
    enc.out_dict_size(dict_size);

    if input.is_empty() {
        enc.out_stream_end();
        return enc.code;
    }

    let mut pos = 0;
    let mut literal_start = 0;

    while pos < input.len() {
        // Find longest match in the dictionary window
        let (match_len, match_dist) = find_best_match(input, pos, dict_size);

        if match_len >= MIN_DICT_MATCH_LEN {
            // Flush pending literals
            if literal_start < pos {
                enc.out_data(&input[literal_start..pos]);
            }
            // dict_pos is 0-based: distance 1 back = dict_pos 0
            enc.out_dict(match_len, match_dist - 1);
            pos += match_len;
            literal_start = pos;
        } else {
            pos += 1;
        }
    }

    // Flush remaining literals
    if literal_start < input.len() {
        enc.out_data(&input[literal_start..]);
    }

    enc.out_stream_end();
    enc.code
}

/// Find the longest match at `pos` looking back up to `dict_size` bytes.
fn find_best_match(data: &[u8], pos: usize, dict_size: usize) -> (usize, usize) {
    let max_lookback = pos.min(dict_size);
    if max_lookback == 0 {
        return (0, 0);
    }

    let remaining = data.len() - pos;
    // Cap match length to what the format supports efficiently
    let max_match = remaining.min(258);

    let mut best_len = 0;
    let mut best_dist = 0;

    // For each possible distance, check match length
    let mut dist = 1;
    while dist <= max_lookback {
        let mut len = 0;
        // Allow overlapping matches (dist < match_len is valid for repeated patterns)
        while len < max_match && data[pos + len] == data[pos - dist + (len % dist)] {
            len += 1;
        }
        if len > best_len && len >= MIN_DICT_MATCH_LEN {
            best_len = len;
            best_dist = dist;
            if len == max_match {
                break;
            }
        }
        dist += 1;
    }

    (best_len, best_dist)
}

/// Decompress tinyuz data. Used for roundtrip testing.
#[cfg(test)]
fn tuz_decompress(compressed: &[u8], original_size: usize) -> Result<Vec<u8>, &'static str> {
    if compressed.len() < 4 {
        return Err("too short");
    }
    // Skip 4-byte dict_size header
    let mut in_pos = 4usize;
    let mut types: u8 = 0;
    let mut type_count: usize = 0;
    let mut out = Vec::with_capacity(original_size);
    let mut dict_pos_back: usize = 1;
    let mut is_have_data_back = false;

    let read_byte = |pos: &mut usize| -> Result<u8, &'static str> {
        if *pos < compressed.len() {
            let b = compressed[*pos];
            *pos += 1;
            Ok(b)
        } else {
            Err("unexpected end of input")
        }
    };

    // Read lowbits from the type bitstream
    let read_lowbits = |types: &mut u8,
                        type_count: &mut usize,
                        in_pos: &mut usize,
                        bit_count: usize|
     -> Result<u8, &'static str> {
        let count = *type_count;
        let result = *types;
        if count >= bit_count {
            *type_count = count - bit_count;
            *types = result >> bit_count;
            Ok(result)
        } else {
            let v = read_byte(in_pos)?;
            let bc = bit_count - count;
            *type_count = MAX_TYPE_BIT_COUNT - bc;
            *types = v >> bc;
            Ok(result | (v << count))
        }
    };

    loop {
        let bit = read_lowbits(&mut types, &mut type_count, &mut in_pos, 1)? & 1;
        if bit == 0 {
            // Dict match or control
            // Unpack len (pack_bit=1): read groups of (1 data bit + 1 continuation bit)
            let mut saved_len: usize = 0;
            loop {
                let lowbit = read_lowbits(&mut types, &mut type_count, &mut in_pos, 2)?;
                saved_len = (saved_len << 1) + (lowbit & 1) as usize;
                if (lowbit & 2) == 0 {
                    break;
                }
                saved_len += 1;
            }

            let saved_dict_pos;
            if is_have_data_back
                && (read_lowbits(&mut types, &mut type_count, &mut in_pos, 1)? & 1) == 1
            {
                saved_dict_pos = dict_pos_back;
            } else {
                // Unpack dict pos
                let b = read_byte(&mut in_pos)?;
                if b < 128 {
                    saved_dict_pos = b as usize;
                } else {
                    // Extended pos: unpack_pos_len (pack_bit=2)
                    let mut pos_ext: usize = 0;
                    loop {
                        let lowbit = read_lowbits(&mut types, &mut type_count, &mut in_pos, 3)?;
                        pos_ext = (pos_ext << 2) + (lowbit & 3) as usize;
                        if (lowbit & 4) == 0 {
                            break;
                        }
                        pos_ext += 1;
                    }
                    saved_dict_pos = ((b as usize & 0x7F) | (pos_ext << 7)) + 128;
                }
                if saved_dict_pos > BIG_POS_FOR_LEN {
                    saved_len += 1;
                }
            }
            is_have_data_back = false;

            if saved_dict_pos != 0 {
                // Back-reference
                let match_len = saved_len + MIN_DICT_MATCH_LEN;
                dict_pos_back = saved_dict_pos;
                let src_start = out
                    .len()
                    .checked_sub(saved_dict_pos)
                    .ok_or("dict pos out of range")?;
                for i in 0..match_len {
                    let b = out[src_start + i];
                    out.push(b);
                }
            } else {
                // Control
                match saved_len {
                    1 => {
                        // literalLine
                        is_have_data_back = true;
                        let mut lit_len: usize = 0;
                        loop {
                            let lowbit = read_lowbits(&mut types, &mut type_count, &mut in_pos, 3)?;
                            lit_len = (lit_len << 2) + (lowbit & 3) as usize;
                            if (lowbit & 4) == 0 {
                                break;
                            }
                            lit_len += 1;
                        }
                        let lit_len = lit_len + MIN_LITERAL_LEN;
                        for _ in 0..lit_len {
                            let b = read_byte(&mut in_pos)?;
                            out.push(b);
                        }
                    }
                    2 => {
                        // clipEnd — reset state and continue
                        dict_pos_back = 1;
                        type_count = 0;
                    }
                    3 => {
                        // streamEnd
                        return Ok(out);
                    }
                    _ => return Err("unknown control type"),
                }
            }
        } else {
            // Literal byte
            is_have_data_back = true;
            let b = read_byte(&mut in_pos)?;
            out.push(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_empty() {
        let result = tuz_compress(&[], 4096);
        // Should have 4-byte dict size header + stream end control
        assert!(result.len() > 4);
        // First 4 bytes = dict_size 4096 = 0x00001000 LE
        assert_eq!(result[0], 0x00);
        assert_eq!(result[1], 0x10);
        assert_eq!(result[2], 0x00);
        assert_eq!(result[3], 0x00);
    }

    #[test]
    fn compress_small_literal() {
        let data = b"hello";
        let result = tuz_compress(data, 4096);
        // Should be larger than input (no matches possible for small data)
        assert!(result.len() > 4);
    }

    #[test]
    fn compress_repetitive_rgb_data() {
        // Simulate static red LED data: 30 frames × 24 LEDs × 3 bytes
        let frame_size = 24 * 3; // 72 bytes per frame
        let total_frames = 30;
        let mut data = Vec::with_capacity(frame_size * total_frames);
        for _ in 0..total_frames {
            for _ in 0..24 {
                data.push(255); // R
                data.push(0); // G
                data.push(0); // B
            }
        }
        assert_eq!(data.len(), 2160);

        let compressed = tuz_compress(&data, 4096);
        // Highly repetitive data should compress very well
        assert!(
            compressed.len() < data.len() / 2,
            "compressed {} bytes to {} bytes — expected >50% compression",
            data.len(),
            compressed.len()
        );
    }

    #[test]
    fn compress_repeated_pattern() {
        // ABCABC... pattern
        let pattern = b"ABC";
        let data: Vec<u8> = pattern.iter().copied().cycle().take(300).collect();
        let compressed = tuz_compress(&data, 4096);
        assert!(
            compressed.len() < data.len(),
            "compressed {} to {} bytes",
            data.len(),
            compressed.len()
        );
    }

    #[test]
    fn roundtrip_small() {
        let data = b"hello world hello world hello";
        let compressed = tuz_compress(data, 4096);
        let decompressed = tuz_decompress(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn roundtrip_rgb_static() {
        // 30 frames × 24 LEDs × 3 bytes of solid red
        let mut data = Vec::new();
        for _ in 0..30 * 24 {
            data.extend_from_slice(&[255, 0, 0]);
        }
        let compressed = tuz_compress(&data, 4096);
        let decompressed = tuz_decompress(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn roundtrip_rgb_multicolor() {
        let colors = [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0)];
        let mut data = Vec::new();
        for frame in 0..30 {
            for led in 0..24 {
                let (r, g, b) = colors[(led + frame) % colors.len()];
                data.push(r);
                data.push(g);
                data.push(b);
            }
        }
        let compressed = tuz_compress(&data, 4096);
        let decompressed = tuz_decompress(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn roundtrip_single_byte() {
        let data = b"x";
        let compressed = tuz_compress(data, 4096);
        let decompressed = tuz_decompress(&compressed, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn compress_rainbow_effect() {
        // Multi-color cycling effect
        let colors = [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0)];
        let mut data = Vec::new();
        for frame in 0..30 {
            for led in 0..24 {
                let (r, g, b) = colors[(led + frame) % colors.len()];
                data.push(r);
                data.push(g);
                data.push(b);
            }
        }
        let compressed = tuz_compress(&data, 4096);
        assert!(
            compressed.len() < data.len(),
            "compressed {} to {} bytes",
            data.len(),
            compressed.len()
        );
    }
}

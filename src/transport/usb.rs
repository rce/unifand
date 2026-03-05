use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};

type DesCbcEnc = cbc::Encryptor<des::Des>;
type DesCbcDec = cbc::Decryptor<des::Des>;

const DES_KEY: &[u8; 8] = b"slv3tuzx";
const CMD_MAGIC: [u8; 2] = [0x1A, 0x6D];
const CMD_BUF_LEN: usize = 504;

/// Encrypt data with DES-CBC. Input is padded to 8-byte boundary with zeros.
pub fn encrypt(plaintext: &[u8]) -> Vec<u8> {
    use des::cipher::generic_array::GenericArray;

    // Pad to 8-byte boundary with zeros
    let padded_len = (plaintext.len() + 7) / 8 * 8;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext.len()].copy_from_slice(plaintext);

    let mut encryptor = DesCbcEnc::new(DES_KEY.into(), DES_KEY.into());
    for chunk in buf.chunks_exact_mut(8) {
        let block = GenericArray::from_mut_slice(chunk);
        encryptor.encrypt_block_mut(block);
    }
    buf
}

/// Encrypt with DES-CBC using PKCS7 padding (for wireless LCD protocol).
/// 504-byte input → 512-byte output (adds 8-byte padding block).
pub fn encrypt_pkcs7(plaintext: &[u8]) -> Vec<u8> {
    use des::cipher::generic_array::GenericArray;

    // PKCS7 padding: if already aligned to 8, add full block of 0x08
    let pad_len = 8 - (plaintext.len() % 8);
    let padded_len = plaintext.len() + pad_len;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext.len()].copy_from_slice(plaintext);
    for byte in &mut buf[plaintext.len()..] {
        *byte = pad_len as u8;
    }

    let mut encryptor = DesCbcEnc::new(DES_KEY.into(), DES_KEY.into());
    for chunk in buf.chunks_exact_mut(8) {
        let block = GenericArray::from_mut_slice(chunk);
        encryptor.encrypt_block_mut(block);
    }
    buf
}

/// Decrypt DES-CBC data.
pub fn decrypt(ciphertext: &[u8]) -> Vec<u8> {
    use des::cipher::generic_array::GenericArray;

    let mut buf = ciphertext.to_vec();
    let mut decryptor = DesCbcDec::new(DES_KEY.into(), DES_KEY.into());
    for chunk in buf.chunks_exact_mut(8) {
        let block = GenericArray::from_mut_slice(chunk);
        decryptor.decrypt_block_mut(block);
    }
    buf
}

/// Build a 504-byte command buffer for wireless protocol.
/// [0]: command, [2-3]: magic 0x1A 0x6D, [4-7]: timestamp (LE u32),
/// [8-11]: data size (BE u32, if data present), [12+]: data payload
pub fn build_command_buffer(cmd: u8, data: Option<&[u8]>) -> [u8; CMD_BUF_LEN] {
    let mut buf = [0u8; CMD_BUF_LEN];
    buf[0] = cmd;
    buf[2] = CMD_MAGIC[0];
    buf[3] = CMD_MAGIC[1];

    // Timestamp as LE u32
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_secs() as u32;
    buf[4..8].copy_from_slice(&ts.to_le_bytes());

    if let Some(data) = data {
        let size = data.len() as u32;
        buf[8..12].copy_from_slice(&size.to_be_bytes());
        buf[12..12 + data.len()].copy_from_slice(data);
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let original = b"Hello, DES-CBC encryption test!";
        let encrypted = encrypt(original);
        // Encrypted data should differ from original
        assert_ne!(&encrypted[..], &original[..]);
        // Encrypted length should be padded to 8-byte boundary
        assert_eq!(encrypted.len() % 8, 0);

        let decrypted = decrypt(&encrypted);
        // Decrypted (may have trailing zero padding) should start with original
        assert!(decrypted.len() >= original.len());
        assert_eq!(&decrypted[..original.len()], &original[..]);
    }

    #[test]
    fn command_buffer_structure() {
        let data = &[0x01, 0x02];
        let buf = build_command_buffer(0x0A, Some(data));

        assert_eq!(buf.len(), CMD_BUF_LEN);
        // Byte 0: command
        assert_eq!(buf[0], 0x0A);
        // Bytes 2-3: magic
        assert_eq!(buf[2], 0x1A);
        assert_eq!(buf[3], 0x6D);
        // Bytes 4-7: timestamp (LE u32) — just check it's nonzero
        let ts = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        assert!(ts > 0, "timestamp should be nonzero");
        // Bytes 8-11: data size as BE u32
        let data_size = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
        assert_eq!(data_size, 2);
        // Bytes 12+: payload
        assert_eq!(buf[12], 0x01);
        assert_eq!(buf[13], 0x02);
    }

    #[test]
    fn command_buffer_no_data() {
        let buf = build_command_buffer(0x0A, None);

        assert_eq!(buf.len(), CMD_BUF_LEN);
        // Byte 0: command
        assert_eq!(buf[0], 0x0A);
        // Bytes 2-3: magic
        assert_eq!(buf[2], 0x1A);
        assert_eq!(buf[3], 0x6D);
        // Bytes 4-7: timestamp should be nonzero
        let ts = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        assert!(ts > 0, "timestamp should be nonzero");
        // Bytes 8-11: no data_size field should be set (all zeros)
        assert_eq!(buf[8], 0);
        assert_eq!(buf[9], 0);
        assert_eq!(buf[10], 0);
        assert_eq!(buf[11], 0);
    }
}

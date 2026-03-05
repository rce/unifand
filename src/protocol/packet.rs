/// 64-byte HID report for fan and LED commands.
/// Report ID 0x01, 6-byte header, max 58-byte payload.

pub const LED_REPORT_ID: u8 = 0x01;
pub const LED_PACKET_LEN: usize = 64;
pub const LED_HEADER_LEN: usize = 6;
pub const LED_MAX_PAYLOAD: usize = LED_PACKET_LEN - LED_HEADER_LEN;

pub struct LedPacket {
    pub command: u8,
    pub packet_number: u16,
    pub data: Vec<u8>,
}

impl LedPacket {
    pub fn new(command: u8, data: Vec<u8>) -> Self {
        Self {
            command,
            packet_number: 0,
            data,
        }
    }

    pub fn to_bytes(&self) -> [u8; LED_PACKET_LEN] {
        let mut buf = [0u8; LED_PACKET_LEN];
        buf[0] = LED_REPORT_ID;
        buf[1] = self.command;
        // buf[2] reserved
        buf[3] = (self.packet_number >> 8) as u8;
        buf[4] = self.packet_number as u8;
        let len = self.data.len().min(LED_MAX_PAYLOAD);
        buf[5] = len as u8;
        buf[6..6 + len].copy_from_slice(&self.data[..len]);
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> crate::Result<Self> {
        if bytes.len() < LED_HEADER_LEN {
            return Err(crate::Error::InvalidResponse(
                "packet too short".into(),
            ));
        }
        if bytes[0] != LED_REPORT_ID {
            return Err(crate::Error::InvalidResponse(format!(
                "expected report ID {:#04x}, got {:#04x}",
                LED_REPORT_ID, bytes[0]
            )));
        }
        let len = bytes[5] as usize;
        let data = bytes[6..6 + len.min(bytes.len() - 6)].to_vec();
        Ok(Self {
            command: bytes[1],
            packet_number: (bytes[3] as u16) << 8 | bytes[4] as u16,
            data,
        })
    }
}

/// 512-byte HID report for LCD commands.
/// Report ID 0x02, 11-byte header, max 501-byte payload.

pub const LCD_REPORT_ID: u8 = 0x02;
pub const LCD_OUTPUT_LEN: usize = 512;
pub const LCD_INPUT_LEN: usize = 64;
pub const LCD_HEADER_LEN: usize = 11;
pub const LCD_MAX_PAYLOAD: usize = LCD_OUTPUT_LEN - LCD_HEADER_LEN;

pub struct LcdPacket {
    pub command: u8,
    pub data_size: u32,
    pub packet_number: u32, // 24-bit on wire
    pub data: Vec<u8>,
}

impl LcdPacket {
    pub fn new(command: u8, data_size: u32, packet_number: u32, data: Vec<u8>) -> Self {
        Self {
            command,
            data_size,
            packet_number,
            data,
        }
    }

    pub fn to_bytes(&self) -> [u8; LCD_OUTPUT_LEN] {
        let mut buf = [0u8; LCD_OUTPUT_LEN];
        buf[0] = LCD_REPORT_ID;
        buf[1] = self.command;
        // data_size as big-endian u32
        let ds = self.data_size.to_be_bytes();
        buf[2..6].copy_from_slice(&ds);
        // packet_number as big-endian 24-bit (3 bytes)
        buf[6] = (self.packet_number >> 16) as u8;
        buf[7] = (self.packet_number >> 8) as u8;
        buf[8] = self.packet_number as u8;
        // payload length as big-endian u16
        let len = self.data.len().min(LCD_MAX_PAYLOAD);
        buf[9] = (len >> 8) as u8;
        buf[10] = len as u8;
        buf[11..11 + len].copy_from_slice(&self.data[..len]);
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> crate::Result<Self> {
        if bytes.len() < LCD_HEADER_LEN {
            return Err(crate::Error::InvalidResponse(
                "packet too short".into(),
            ));
        }
        if bytes[0] != LCD_REPORT_ID {
            return Err(crate::Error::InvalidResponse(format!(
                "expected report ID {:#04x}, got {:#04x}",
                LCD_REPORT_ID, bytes[0]
            )));
        }
        let data_size = u32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
        let packet_number =
            (bytes[6] as u32) << 16 | (bytes[7] as u32) << 8 | bytes[8] as u32;
        let payload_len = ((bytes[9] as usize) << 8 | bytes[10] as usize)
            .min(bytes.len() - LCD_HEADER_LEN);
        let data = bytes[11..11 + payload_len].to_vec();
        Ok(Self {
            command: bytes[1],
            data_size,
            packet_number,
            data,
        })
    }

    pub fn build_packets(command: u8, data: &[u8]) -> Vec<Self> {
        let total_size = data.len() as u32;
        data.chunks(LCD_MAX_PAYLOAD)
            .enumerate()
            .map(|(i, chunk)| Self {
                command,
                data_size: total_size,
                packet_number: i as u32,
                data: chunk.to_vec(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn led_packet_serializes_correctly() {
        let pkt = LedPacket {
            command: 0xAA,
            packet_number: 0,
            data: vec![0x12, 0x80],
        };
        let bytes = pkt.to_bytes();
        assert_eq!(bytes.len(), LED_PACKET_LEN);
        assert_eq!(bytes[0], LED_REPORT_ID);
        assert_eq!(bytes[1], 0xAA);
        assert_eq!(bytes[2], 0x00); // reserved
        assert_eq!(bytes[3], 0x00); // packet_number high
        assert_eq!(bytes[4], 0x00); // packet_number low
        assert_eq!(bytes[5], 2);    // payload length
        assert_eq!(bytes[6], 0x12);
        assert_eq!(bytes[7], 0x80);
        assert_eq!(bytes[8], 0x00); // rest is zero-padded
    }

    #[test]
    fn led_packet_parses_response() {
        let mut bytes = [0u8; LED_PACKET_LEN];
        bytes[0] = LED_REPORT_ID;
        bytes[1] = 0xA1;
        bytes[5] = 3;
        bytes[6] = 0xDE;
        bytes[7] = 0xAD;
        bytes[8] = 0xBE;
        let pkt = LedPacket::from_bytes(&bytes).unwrap();
        assert_eq!(pkt.command, 0xA1);
        assert_eq!(pkt.data, vec![0xDE, 0xAD, 0xBE]);
    }

    #[test]
    fn led_packet_rejects_wrong_report_id() {
        let mut bytes = [0u8; LED_PACKET_LEN];
        bytes[0] = 0x02; // wrong
        assert!(LedPacket::from_bytes(&bytes).is_err());
    }

    #[test]
    fn lcd_packet_serializes_correctly() {
        let pkt = LcdPacket {
            command: 0x41,
            data_size: 1500,
            packet_number: 0,
            data: vec![0xFF; 501],
        };
        let bytes = pkt.to_bytes();
        assert_eq!(bytes.len(), LCD_OUTPUT_LEN);
        assert_eq!(bytes[0], LCD_REPORT_ID);
        assert_eq!(bytes[1], 0x41);
        // data_size 1500 = 0x000005DC big-endian
        assert_eq!(bytes[2], 0x00);
        assert_eq!(bytes[3], 0x00);
        assert_eq!(bytes[4], 0x05);
        assert_eq!(bytes[5], 0xDC);
        // packet_number 0
        assert_eq!(bytes[6], 0x00);
        assert_eq!(bytes[7], 0x00);
        assert_eq!(bytes[8], 0x00);
        // length 501 = 0x01F5
        assert_eq!(bytes[9], 0x01);
        assert_eq!(bytes[10], 0xF5);
        assert_eq!(bytes[11], 0xFF);
    }

    #[test]
    fn lcd_packet_chunks_large_data() {
        let data = vec![0xAB; 1200]; // needs 3 packets (501 + 501 + 198)
        let packets = LcdPacket::build_packets(0x41, &data);
        assert_eq!(packets.len(), 3);
        assert_eq!(packets[0].data_size, 1200);
        assert_eq!(packets[0].packet_number, 0);
        assert_eq!(packets[0].data.len(), 501);
        assert_eq!(packets[1].packet_number, 1);
        assert_eq!(packets[1].data.len(), 501);
        assert_eq!(packets[2].packet_number, 2);
        assert_eq!(packets[2].data.len(), 198);
    }

    #[test]
    fn lcd_packet_parses_input_response() {
        let mut bytes = [0u8; LCD_INPUT_LEN];
        bytes[0] = LCD_REPORT_ID;
        bytes[1] = 0x3C;
        bytes[9] = 0x00;
        bytes[10] = 3;
        bytes[11] = 0x01;
        bytes[12] = 0x00;
        bytes[13] = 0x05;
        let pkt = LcdPacket::from_bytes(&bytes).unwrap();
        assert_eq!(pkt.command, 0x3C);
        assert_eq!(pkt.data, vec![0x01, 0x00, 0x05]);
    }
}

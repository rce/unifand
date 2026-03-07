use crate::protocol::packet::LcdPacket;

pub mod commands {
    pub const GET_HANDSHAKE_INFO: u8 = 0x3C;
    pub const GET_PRODUCT_INFO: u8 = 0x3D;
    pub const READ_SERIAL_NUMBER: u8 = 0x3E;
    pub const LCD_CONTROL: u8 = 0x40;
    pub const WRITE_JPG: u8 = 0x41;
    pub const WRITE_AVI: u8 = 0x45;
    pub const WRITE_SYNC_JPG: u8 = 0x46;
    pub const WRITE_BOOT_AVI: u8 = 0x47;
    pub const WRITE_BOOT_JPG: u8 = 0x48;
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum LcdMode {
    ShowJpg = 1,
    ShowAvi = 3,
    ShowAppSync = 4,
    LcdSetting = 5,
    LcdTest = 6,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum ScreenRotation {
    Deg0 = 0,
    Deg90 = 1,
    Deg180 = 2,
    Deg270 = 3,
}

pub struct LcdControlSetting {
    pub mode: LcdMode,
    pub jpg_index: u16,
    pub brightness: u8,
    pub video_fps: u8,
    pub rotation: ScreenRotation,
    pub enable_test: bool,
    pub test_color: (u8, u8, u8),
}

pub struct HandshakeInfo {
    pub mode: u8,
    pub frame_index: u16,
}

pub struct SerialNumberInfo {
    pub serial: String,
    pub port: u8,
    pub lcd_index: u8,
}

impl LcdControlSetting {
    /// Encode as 11 bytes: [mode, jpg_hi, jpg_lo, 0, brightness, fps, rotation, test_enable, R, G, B]
    pub fn to_bytes(&self) -> Vec<u8> {
        vec![
            self.mode as u8,
            (self.jpg_index >> 8) as u8,
            self.jpg_index as u8,
            0,
            self.brightness,
            self.video_fps,
            self.rotation as u8,
            self.enable_test as u8,
            self.test_color.0,
            self.test_color.1,
            self.test_color.2,
        ]
    }
}

/// Build LCD control packet(s) for the given setting.
pub fn lcd_control_packet(setting: &LcdControlSetting) -> Vec<LcdPacket> {
    LcdPacket::build_packets(commands::LCD_CONTROL, &setting.to_bytes())
}

/// Build a handshake request (command 0x3C, empty data).
pub fn handshake_packet() -> Vec<LcdPacket> {
    vec![LcdPacket::new(commands::GET_HANDSHAKE_INFO, 0, 0, vec![])]
}

/// Build packets for JPEG image data.
pub fn write_jpg_packets(jpg_data: &[u8]) -> Vec<LcdPacket> {
    LcdPacket::build_packets(commands::WRITE_JPG, jpg_data)
}

/// Build packets for sync JPEG image data.
pub fn write_sync_jpg_packets(jpg_data: &[u8]) -> Vec<LcdPacket> {
    LcdPacket::build_packets(commands::WRITE_SYNC_JPG, jpg_data)
}

/// Build packets for AVI/H.264 video data.
pub fn write_avi_packets(h264_data: &[u8]) -> Vec<LcdPacket> {
    LcdPacket::build_packets(commands::WRITE_AVI, h264_data)
}

/// Parse a handshake response: [mode, frame_hi, frame_lo].
pub fn parse_handshake(data: &[u8]) -> crate::Result<HandshakeInfo> {
    if data.len() < 3 {
        return Err(crate::Error::InvalidResponse(format!(
            "handshake response too short: {} bytes",
            data.len()
        )));
    }
    Ok(HandshakeInfo {
        mode: data[0],
        frame_index: (data[1] as u16) << 8 | data[2] as u16,
    })
}

/// Parse a serial number response: [32 bytes serial, port, lcd_index].
pub fn parse_serial_number(data: &[u8]) -> crate::Result<SerialNumberInfo> {
    if data.len() < 34 {
        return Err(crate::Error::InvalidResponse(format!(
            "serial number response too short: {} bytes",
            data.len()
        )));
    }
    let serial = String::from_utf8_lossy(&data[..32])
        .trim_end_matches('\0')
        .to_string();
    Ok(SerialNumberInfo {
        serial,
        port: data[32],
        lcd_index: data[33],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcd_control_setting_encoding() {
        let setting = LcdControlSetting {
            mode: LcdMode::ShowJpg,
            jpg_index: 5,
            brightness: 80,
            video_fps: 30,
            rotation: ScreenRotation::Deg180,
            enable_test: false,
            test_color: (0, 0, 0),
        };
        let bytes = setting.to_bytes();
        assert_eq!(bytes.len(), 11);
        assert_eq!(bytes[0], 1); // ShowJpg
        assert_eq!(bytes[1], 0); // jpg_index high
        assert_eq!(bytes[2], 5); // jpg_index low
        assert_eq!(bytes[3], 0); // reserved
        assert_eq!(bytes[4], 80); // brightness
        assert_eq!(bytes[5], 30); // fps
        assert_eq!(bytes[6], 2); // Deg180
        assert_eq!(bytes[7], 0); // enable_test = false
        assert_eq!(bytes[8], 0);
        assert_eq!(bytes[9], 0);
        assert_eq!(bytes[10], 0);
    }

    #[test]
    fn write_jpg_creates_correct_packets() {
        let data = vec![0xAB; 1000];
        let packets = write_jpg_packets(&data);
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].command, commands::WRITE_JPG);
        assert_eq!(packets[0].data.len(), 501);
        assert_eq!(packets[1].data.len(), 499);
        assert_eq!(packets[0].data_size, 1000);
        assert_eq!(packets[1].data_size, 1000);
        assert_eq!(packets[0].packet_number, 0);
        assert_eq!(packets[1].packet_number, 1);
    }

    #[test]
    fn parse_handshake_info() {
        // mode=1, frame_index=10 (0x000A big-endian)
        let data = vec![1, 0x00, 0x0A];
        let info = parse_handshake(&data).unwrap();
        assert_eq!(info.mode, 1);
        assert_eq!(info.frame_index, 10);
    }

    #[test]
    fn parse_serial_number_info() {
        let mut data = vec![0u8; 34];
        // Write "SN123" into the first 32 bytes
        data[..5].copy_from_slice(b"SN123");
        data[32] = 2; // port
        data[33] = 1; // lcd_index
        let info = parse_serial_number(&data).unwrap();
        assert_eq!(info.serial, "SN123");
        assert_eq!(info.port, 2);
        assert_eq!(info.lcd_index, 1);
    }

    #[test]
    fn parse_handshake_rejects_short_data() {
        assert!(parse_handshake(&[1, 2]).is_err());
    }

    #[test]
    fn parse_serial_number_rejects_short_data() {
        assert!(parse_serial_number(&[0u8; 33]).is_err());
    }

    #[test]
    fn handshake_packet_has_correct_command() {
        let packets = handshake_packet();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].command, commands::GET_HANDSHAKE_INFO);
        assert!(packets[0].data.is_empty());
    }

    #[test]
    fn lcd_control_packet_wraps_setting() {
        let setting = LcdControlSetting {
            mode: LcdMode::ShowAvi,
            jpg_index: 0,
            brightness: 50,
            video_fps: 24,
            rotation: ScreenRotation::Deg0,
            enable_test: false,
            test_color: (0, 0, 0),
        };
        let packets = lcd_control_packet(&setting);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].command, commands::LCD_CONTROL);
        assert_eq!(packets[0].data.len(), 11);
    }
}

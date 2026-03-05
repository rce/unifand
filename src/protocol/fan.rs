use crate::protocol::packet::LedPacket;

pub mod commands {
    pub const HANDSHAKE: u8 = 0xA1;
    pub const SET_FAN_SPEED: u8 = 0xAA;
    pub const GET_PRODUCT_INFO: u8 = 0xA6;
    pub const SET_MB_FAN_SYNC: u8 = 0xB1;
    pub const SET_FAN_GROUPING: u8 = 0xAD;
    pub const SET_FAN_DIRECTION: u8 = 0xAE;
    pub const SET_PORT_DIRECTION: u8 = 0xAF;
    pub const BLINK_PORT: u8 = 0xB4;
}

#[derive(Debug, Clone)]
pub struct FanInfo {
    pub detected: bool,
    pub upgrading: bool,
    pub port: u8,
    pub fan_index: u8,
    pub rpm: u16,
}

pub fn handshake_packet() -> LedPacket {
    LedPacket::new(commands::HANDSHAKE, vec![])
}

pub fn parse_handshake(data: &[u8]) -> Vec<FanInfo> {
    data.chunks_exact(3)
        .map(|chunk| {
            let flags = chunk[0];
            FanInfo {
                detected: flags & 0x80 != 0,
                upgrading: flags & 0x40 != 0,
                port: (flags >> 4) & 0x03,
                fan_index: flags & 0x0F,
                rpm: u16::from_be_bytes([chunk[1], chunk[2]]),
            }
        })
        .collect()
}

pub fn set_fan_speed_packet(port: u8, fan_index: u8, pwm: u8) -> LedPacket {
    LedPacket::new(commands::SET_FAN_SPEED, vec![(port << 4) | fan_index, pwm])
}

pub fn set_mb_sync_packet(port: u8, fan_index: u8, sync: bool) -> LedPacket {
    let sync_bit = if sync { 1u8 } else { 0u8 };
    LedPacket::new(
        commands::SET_MB_FAN_SYNC,
        vec![(sync_bit << 7) | (port << 4) | fan_index],
    )
}

pub fn blink_port_packet(port: u8) -> LedPacket {
    LedPacket::new(commands::BLINK_PORT, vec![port])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_packet_correct() {
        let pkt = handshake_packet();
        assert_eq!(pkt.command, commands::HANDSHAKE);
        assert!(pkt.data.is_empty());
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[1], 0xA1);
        assert_eq!(bytes[5], 0); // payload length 0
    }

    #[test]
    fn parse_handshake_response() {
        // port 1, fan 2, detected=true, upgrading=false, RPM=1200
        // flags: 1_0_01_0010 = 0x92
        // RPM 1200 = 0x04B0
        let data = [0x92, 0x04, 0xB0];
        let fans = parse_handshake(&data);
        assert_eq!(fans.len(), 1);
        assert!(fans[0].detected);
        assert!(!fans[0].upgrading);
        assert_eq!(fans[0].port, 1);
        assert_eq!(fans[0].fan_index, 2);
        assert_eq!(fans[0].rpm, 1200);
    }

    #[test]
    fn parse_handshake_multiple_fans() {
        let data = [
            // Fan 1: port 0, fan 0, detected, not upgrading, 800 RPM
            0x80, 0x03, 0x20,
            // Fan 2: port 2, fan 1, detected, upgrading, 0 RPM
            0xE1, 0x00, 0x00,
            // Fan 3: port 1, fan 3, not detected, not upgrading, 0 RPM
            0x13, 0x00, 0x00,
        ];
        let fans = parse_handshake(&data);
        assert_eq!(fans.len(), 3);

        assert!(fans[0].detected);
        assert!(!fans[0].upgrading);
        assert_eq!(fans[0].port, 0);
        assert_eq!(fans[0].fan_index, 0);
        assert_eq!(fans[0].rpm, 800);

        assert!(fans[1].detected);
        assert!(fans[1].upgrading);
        assert_eq!(fans[1].port, 2);
        assert_eq!(fans[1].fan_index, 1);
        assert_eq!(fans[1].rpm, 0);

        assert!(!fans[2].detected);
        assert!(!fans[2].upgrading);
        assert_eq!(fans[2].port, 1);
        assert_eq!(fans[2].fan_index, 3);
        assert_eq!(fans[2].rpm, 0);
    }

    #[test]
    fn set_fan_speed_encoding() {
        let pkt = set_fan_speed_packet(2, 3, 128);
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[1], commands::SET_FAN_SPEED);
        assert_eq!(bytes[6], 0x23); // (2<<4)|3
        assert_eq!(bytes[7], 128);
    }

    #[test]
    fn set_mb_sync_encoding() {
        let pkt = set_mb_sync_packet(1, 0, true);
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[1], commands::SET_MB_FAN_SYNC);
        assert_eq!(bytes[6], 0x90); // (1<<7)|(1<<4)|0
    }
}

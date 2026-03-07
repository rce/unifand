use crate::protocol::packet::LedPacket;

pub mod commands {
    pub const SET_FAN_LIGHT: u8 = 0xA3;
    pub const SET_FAN_GROUP_LIGHT: u8 = 0xB0;
    pub const SET_FAN_GROUPING: u8 = 0xAD;
    pub const SET_FAN_DIRECTION: u8 = 0xAE;
    pub const TEST_LIGHT: u8 = 0xB3;
}

#[derive(Debug, Clone, Copy)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum LightingDirection {
    RightOrClockwise = 0,
    LeftOrCounterClockwise = 1,
    Up = 2,
    Down = 3,
    Spreaded = 4,
    Gathered = 5,
}

pub struct FanLightConfig {
    pub port: u8,
    pub fan_index: u8,
    pub sync: bool,
    pub mode: u8,
    pub brightness: u8,
    pub speed: u8,
    pub colors: Vec<Rgb>,
    pub direction: LightingDirection,
    pub disabled: bool,
}

pub fn set_fan_light_packet(cfg: &FanLightConfig) -> LedPacket {
    let mut payload = vec![0u8; 20];
    let sync_bit = if cfg.sync { 1u8 } else { 0u8 };
    payload[0] = (cfg.port << 4) | sync_bit;
    payload[1] = (cfg.port << 4) | cfg.fan_index;
    payload[2] = cfg.mode;
    payload[3] = cfg.brightness;
    payload[4] = cfg.speed;
    // Up to 4 RGB colors at bytes 5-16
    for (i, color) in cfg.colors.iter().take(4).enumerate() {
        let offset = 5 + i * 3;
        payload[offset] = color.r;
        payload[offset + 1] = color.g;
        payload[offset + 2] = color.b;
    }
    payload[17] = cfg.direction as u8;
    payload[18] = if cfg.disabled { 1 } else { 0 };
    payload[19] = cfg.colors.len().min(4) as u8;
    LedPacket::new(commands::SET_FAN_LIGHT, payload)
}

pub fn set_fan_direction_packet(
    port: u8,
    fan_index: u8,
    swap_top_bottom: bool,
    swap_left_right: bool,
) -> LedPacket {
    let flags = ((swap_top_bottom as u8) << 1) | (swap_left_right as u8);
    LedPacket::new(
        commands::SET_FAN_DIRECTION,
        vec![(port << 4) | fan_index, flags],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_fan_light_encoding() {
        let cfg = FanLightConfig {
            port: 0,
            fan_index: 0,
            sync: false,
            mode: 1,       // static
            brightness: 4, // full
            speed: 0,
            colors: vec![Rgb { r: 255, g: 0, b: 0 }],
            direction: LightingDirection::RightOrClockwise,
            disabled: false,
        };
        let pkt = set_fan_light_packet(&cfg);
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[1], commands::SET_FAN_LIGHT);
        assert_eq!(bytes[5], 20); // payload length
        // payload starts at byte 6
        assert_eq!(bytes[6], 0x00); // (0<<4)|0 sync=false
        assert_eq!(bytes[7], 0x00); // (0<<4)|0 fan_index
        assert_eq!(bytes[8], 1); // mode
        assert_eq!(bytes[9], 4); // brightness
        assert_eq!(bytes[10], 0); // speed
        assert_eq!(bytes[11], 255); // R
        assert_eq!(bytes[12], 0); // G
        assert_eq!(bytes[13], 0); // B
        assert_eq!(bytes[23], 0); // direction
        assert_eq!(bytes[24], 0); // disabled
        assert_eq!(bytes[25], 1); // color count
    }

    #[test]
    fn set_fan_direction_encoding() {
        let pkt = set_fan_direction_packet(1, 2, true, false);
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[1], commands::SET_FAN_DIRECTION);
        assert_eq!(bytes[6], 0x12); // (1<<4)|2
        assert_eq!(bytes[7], 0x02); // swap_top_bottom=1 << 1 = 2
    }
}

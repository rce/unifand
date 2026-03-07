/// SLV3H wireless hub protocol.
///
/// The wireless Lian Li fan system uses three USB devices:
/// - SLV3H Hub (1A86:2107) — HID, for MAC address lookup
/// - RF TX Dongle (0416:8040) — raw USB, sends commands & queries master
/// - RF RX Dongle (0416:8041) — raw USB, receives device list/status
///
/// RF packets are 240 bytes, fragmented into 60-byte USB chunks.

/// HID command to get the hub's MAC address.
pub const CMD_GET_MAC: u8 = 0x1C; // 28

/// USB command to query master MAC and firmware version (sent to TX dongle).
pub const CMD_QUERY_MASTER: u8 = 0x11; // 17

/// USB command to send/receive RF data (fragmented 240-byte packets).
pub const CMD_SEND_RF: u8 = 0x10; // 16

/// USB command to reset the peer dongle.
pub const CMD_RESET: u8 = 0x15; // 21

/// USB command to close USB connection.
pub const CMD_CLOSE: u8 = 0x16; // 22

/// RF sub-commands (byte[1] of the 240-byte RF packet, byte[0] is always 0x12).
pub const RF_CMD: u8 = 0x12; // 18 — RF command envelope
pub const RF_BIND: u8 = 0x10; // 16 — bind/configure/set PWM
pub const RF_SELECT: u8 = 0x12; // 18 — select device (blink)
pub const RF_PRINT: u8 = 0x13; // 19 — print/query device info
pub const RF_CLOCK_SYNC: u8 = 0x14; // 20 — sync system clock
pub const RF_SAVE_CFG: u8 = 0x15; // 21 — save configuration
pub const RF_REBOOT_LCD: u8 = 0x16; // 22 — reboot LCD display
pub const RF_SWITCH_THEME: u8 = 0x19; // 25 — switch wireless theme
pub const RF_RGB_SYNC: u8 = 0x20; // 32 — sync RGB effect data
pub const RF_UPDATE_AIO: u8 = 0x21; // 33 — update AIO/water block params
pub const RF_SEND_PIC: u8 = 0x22; // 34 — send picture data
pub const RF_CLOSE_WIFI: u8 = 0x23; // 35 — close WiFi
pub const RF_MB_SYNC_SWITCH: u8 = 0x24; // 36 — motherboard sync switch
pub const RF_LIGHT_SYNC_SWITCH: u8 = 0x26; // 38 — light sync switch

pub const RF_PACKET_LEN: usize = 240;
pub const RF_CHUNK_LEN: usize = 60;
pub const USB_PACKET_LEN: usize = 64;

/// Default RF channel.
pub const DEFAULT_CHANNEL: u8 = 8;

/// Bytes per device entry in the device list response.
pub const DEVICE_ENTRY_LEN: usize = 42;

/// Default page length for device list responses.
pub const PAGE_LENGTH: usize = 434;

/// Device types from the SLV3H protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Slv3DevType {
    Slv3Fan = 0,
    Strimer8Pin1 = 1,
    Strimer8Pin2 = 2,
    Strimer24Pin1 = 3,
    Strimer24Pin2 = 4,
    WaterBlock = 10,
    WaterBlock2 = 11,
    Lc217 = 65,
    V150 = 66,
    Unknown = 255,
}

impl From<u8> for Slv3DevType {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::Slv3Fan,
            1 => Self::Strimer8Pin1,
            2 => Self::Strimer8Pin2,
            3 => Self::Strimer24Pin1,
            4 => Self::Strimer24Pin2,
            10 => Self::WaterBlock,
            11 => Self::WaterBlock2,
            65 => Self::Lc217,
            66 => Self::V150,
            255 => Self::Unknown,
            _ => Self::Unknown,
        }
    }
}

/// Information about the master hub.
#[derive(Debug, Clone)]
pub struct MasterInfo {
    pub mac: [u8; 6],
    pub sys_clock: u64,
    pub firmware_version: u16,
}

/// A device discovered via RF.
#[derive(Debug, Clone)]
pub struct RfDeviceInfo {
    pub mac: [u8; 6],
    pub master_mac: [u8; 6],
    pub channel: u8,
    pub rx_type: u8,
    pub dev_type: Slv3DevType,
    pub fan_num: u8,
    pub effect_index: [u8; 4],
    pub fan_types: [u8; 4],
    pub fan_speeds: [u16; 4],
    pub fan_pwm: [u8; 4],
    pub cmd_seq: u8,
    pub bound: bool,
}

/// Build a 64-byte HID packet for SLV3H hub (Report ID 0).
pub fn get_mac_packet() -> [u8; USB_PACKET_LEN] {
    let mut buf = [0u8; USB_PACKET_LEN];
    buf[0] = CMD_GET_MAC;
    buf
}

/// Parse MAC address from SLV3H hub response.
/// Response: byte[0]=echo cmd, bytes[1..7]=MAC
pub fn parse_mac_response(data: &[u8]) -> Option<[u8; 6]> {
    if data.len() < 7 {
        return None;
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&data[1..7]);
    // Check for empty MAC
    if mac == [0u8; 6] {
        return None;
    }
    Some(mac)
}

/// Build query master MAC packet (sent to TX dongle).
pub fn query_master_packet(channel: u8) -> [u8; USB_PACKET_LEN] {
    let mut buf = [0u8; USB_PACKET_LEN];
    buf[0] = CMD_QUERY_MASTER;
    buf[1] = channel;
    buf
}

/// Parse master MAC response from TX dongle.
/// byte[0]=0x11, bytes[1..7]=MAC, bytes[7..11]=sys_clock, bytes[11..13]=firmware
pub fn parse_master_response(data: &[u8]) -> Option<MasterInfo> {
    if data.len() < 13 || data[0] != CMD_QUERY_MASTER {
        return None;
    }
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&data[1..7]);
    if mac == [0u8; 6] {
        return None;
    }
    let time_tmos = ((data[7] as u64) << 24)
        | ((data[8] as u64) << 16)
        | ((data[9] as u64) << 8)
        | data[10] as u64;
    let sys_clock = (time_tmos as f64 * 0.625) as u64;
    if sys_clock == 0 {
        return None;
    }
    let firmware_version = ((data[11] as u16) << 8) | data[12] as u16;
    Some(MasterInfo {
        mac,
        sys_clock,
        firmware_version,
    })
}

/// Build a get-device-list packet (sent to RX dongle).
pub fn get_device_list_packet(page_count: u8) -> [u8; USB_PACKET_LEN] {
    let mut buf = [0u8; USB_PACKET_LEN];
    buf[0] = CMD_SEND_RF;
    buf[1] = page_count;
    buf
}

/// Parse device list from RX dongle response.
/// Response format: byte[0]=0x10, byte[1]=device_count,
/// then 42-byte entries starting at offset 4.
pub fn parse_device_list(data: &[u8], master_mac: &[u8; 6]) -> Vec<RfDeviceInfo> {
    if data.is_empty() || data[0] != CMD_SEND_RF {
        return Vec::new();
    }
    let count = data[1] as usize;
    if count == 0 {
        return Vec::new();
    }

    let mut devices = Vec::new();
    let mut offset = 4;

    for _ in 0..count {
        if offset + DEVICE_ENTRY_LEN > data.len() {
            break;
        }
        // Check marker byte at offset+41
        if data[offset + 41] != CMD_GET_MAC {
            offset += DEVICE_ENTRY_LEN;
            continue;
        }

        let mut mac = [0u8; 6];
        mac.copy_from_slice(&data[offset..offset + 6]);

        let mut dev_master_mac = [0u8; 6];
        dev_master_mac.copy_from_slice(&data[offset + 6..offset + 12]);

        let channel = data[offset + 12];
        let rx_type = data[offset + 13];
        let dev_type = Slv3DevType::from(data[offset + 18]);
        let mut fan_num = data[offset + 19];
        if fan_num >= 10 {
            fan_num -= 10;
        }

        let mut effect_index = [0u8; 4];
        effect_index.copy_from_slice(&data[offset + 20..offset + 24]);

        let mut fan_types = [0u8; 4];
        fan_types.copy_from_slice(&data[offset + 24..offset + 28]);

        let mut fan_speeds = [0u16; 4];
        for i in 0..4 {
            fan_speeds[i] =
                ((data[offset + 28 + i * 2] as u16) << 8) | data[offset + 29 + i * 2] as u16;
        }

        let mut fan_pwm = [0u8; 4];
        fan_pwm.copy_from_slice(&data[offset + 36..offset + 40]);

        let cmd_seq = data[offset + 40];
        let bound = dev_master_mac == *master_mac;

        devices.push(RfDeviceInfo {
            mac,
            master_mac: dev_master_mac,
            channel,
            rx_type,
            dev_type,
            fan_num,
            effect_index,
            fan_types,
            fan_speeds,
            fan_pwm,
            cmd_seq,
            bound,
        });

        offset += DEVICE_ENTRY_LEN;
    }

    devices
}

/// Fragment a 240-byte RF packet into 64-byte USB chunks for sending via TX dongle.
/// Each chunk: [0]=0x10, [1]=fragment_index, [2]=channel, [3]=rx_type, [4..64]=60 bytes of RF data
pub fn fragment_rf_packet(
    rf_data: &[u8; RF_PACKET_LEN],
    channel: u8,
    rx_type: u8,
) -> Vec<[u8; USB_PACKET_LEN]> {
    let mut chunks = Vec::new();
    let mut offset = 0;
    let mut frag_idx: u8 = 0;

    while offset < RF_PACKET_LEN {
        let mut chunk = [0u8; USB_PACKET_LEN];
        chunk[0] = CMD_SEND_RF;
        chunk[1] = frag_idx;
        chunk[2] = channel;
        chunk[3] = rx_type;
        let len = RF_CHUNK_LEN.min(RF_PACKET_LEN - offset);
        chunk[4..4 + len].copy_from_slice(&rf_data[offset..offset + len]);
        chunks.push(chunk);
        offset += RF_CHUNK_LEN;
        frag_idx += 1;
    }

    chunks
}

/// Build an RF bind/PWM packet.
/// This is the main fan control command — sets fan PWM values and binds/unbinds devices.
pub fn build_rf_bind_packet(
    target_mac: &[u8; 6],
    master_mac: &[u8; 6],
    target_rx_type: u8,
    target_channel: u8,
    slave_index: u8,
    fan_pwm: &[u8; 4],
) -> [u8; RF_PACKET_LEN] {
    let mut rf = [0u8; RF_PACKET_LEN];
    rf[0] = RF_CMD;
    rf[1] = RF_BIND;
    rf[2..8].copy_from_slice(target_mac);
    rf[8..14].copy_from_slice(master_mac);
    rf[14] = target_rx_type;
    rf[15] = target_channel;
    rf[16] = slave_index; // 0 = unbind
    rf[17..21].copy_from_slice(fan_pwm);
    rf
}

/// Build an RF save-config broadcast packet.
pub fn build_rf_save_config_packet(master_mac: &[u8; 6]) -> [u8; RF_PACKET_LEN] {
    let mut rf = [0u8; RF_PACKET_LEN];
    rf[0] = RF_CMD;
    rf[1] = RF_SAVE_CFG;
    rf[2..8].copy_from_slice(&[0xFF; 6]); // broadcast
    rf[8..14].copy_from_slice(master_mac);
    rf[14] = 0xFF;
    rf
}

/// Build an RF clock sync packet.
pub fn build_rf_clock_sync_packet(master_mac: &[u8; 6], cpu_info: &[u8]) -> [u8; RF_PACKET_LEN] {
    let mut rf = [0u8; RF_PACKET_LEN];
    rf[0] = RF_CMD;
    rf[1] = RF_CLOCK_SYNC;
    rf[8..14].copy_from_slice(master_mac);
    let len = cpu_info.len().min(RF_PACKET_LEN - 14);
    rf[14..14 + len].copy_from_slice(&cpu_info[..len]);
    rf
}

/// Maximum compressed RGB data per RF packet (bytes 20-239).
const RGB_DATA_PER_PACKET: usize = 220;

/// LEDs per SL wireless fan (26 per fan, confirmed via pcap).
pub const LEDS_PER_FAN: usize = 26;

/// Default number of frames for a static effect (70, confirmed via pcap).
pub const STATIC_FRAMES: usize = 70;

/// Build RF_RGB_SYNC (0x20) packet sequence for LED effects.
///
/// The wireless LED protocol sends pre-rendered, tinyuz-compressed RGB
/// frame data as a multi-packet sequence:
/// - Packet 0: metadata (data length, frame count, LED count, timing)
/// - Packets 1..N: compressed RGB data chunks (220 bytes each)
///
/// Returns a list of 240-byte RF packets to send in order.
pub fn build_rf_rgb_sync_packets(
    target_mac: &[u8; 6],
    master_mac: &[u8; 6],
    effect_index: &[u8; 4],
    compressed_data: &[u8],
    total_frame: u16,
    led_num: u8,
    interval_ms: f64,
) -> Vec<[u8; RF_PACKET_LEN]> {
    let data_len = compressed_data.len();
    let data_packets = (data_len + RGB_DATA_PER_PACKET - 1) / RGB_DATA_PER_PACKET;
    let total_packet_count = (data_packets + 1) as u8; // +1 for metadata packet

    let mut packets = Vec::new();

    // Packet 0: metadata
    let mut rf = [0u8; RF_PACKET_LEN];
    rf[0] = RF_CMD;
    rf[1] = RF_RGB_SYNC;
    rf[2..8].copy_from_slice(target_mac);
    rf[8..14].copy_from_slice(master_mac);
    rf[14..18].copy_from_slice(effect_index);
    rf[18] = 0; // packet_index = 0
    rf[19] = total_packet_count;
    // data_length (big-endian u32)
    rf[20] = (data_len >> 24) as u8;
    rf[21] = ((data_len >> 16) & 0xFF) as u8;
    rf[22] = ((data_len >> 8) & 0xFF) as u8;
    rf[23] = (data_len & 0xFF) as u8;
    // rf[24] = 0 (reserved)
    // total_frame (big-endian u16)
    rf[25] = (total_frame >> 8) as u8;
    rf[26] = (total_frame & 0xFF) as u8;
    // led_num
    rf[27] = led_num;
    // interval (big-endian u16 integer part + fractional byte)
    let interval_int = interval_ms as u16;
    rf[32] = (interval_int >> 8) as u8;
    rf[33] = (interval_int & 0xFF) as u8;
    rf[34] = (interval_ms * 100.0 % 100.0) as u8;
    // sub_interval = 0 (bytes 35-36)
    // isOuterMatchMax = 0 (byte 37)
    // total_sub_frame = 0 (bytes 38-39)
    packets.push(rf);

    // Packets 1..N: compressed data
    let mut offset = 0;
    let mut pkt_index: u8 = 1;
    while offset < data_len {
        let mut rf = [0u8; RF_PACKET_LEN];
        rf[0] = RF_CMD;
        rf[1] = RF_RGB_SYNC;
        rf[2..8].copy_from_slice(target_mac);
        rf[8..14].copy_from_slice(master_mac);
        rf[14..18].copy_from_slice(effect_index);
        rf[18] = pkt_index;
        rf[19] = total_packet_count;
        let chunk_len = RGB_DATA_PER_PACKET.min(data_len - offset);
        rf[20..20 + chunk_len].copy_from_slice(&compressed_data[offset..offset + chunk_len]);
        packets.push(rf);
        offset += chunk_len;
        pkt_index += 1;
    }

    packets
}

/// Render a static color effect as raw RGB data.
///
/// Returns (rgb_data, total_frames, led_num) for use with tinyuz compression.
/// Layout: for each frame, for each LED: [R, G, B].
pub fn render_static_rgb(colors: &[(u8, u8, u8)], fan_num: u8) -> Vec<u8> {
    let led_num = LEDS_PER_FAN * fan_num as usize;
    let total_frames = STATIC_FRAMES;
    let mut data = Vec::with_capacity(led_num * 3 * total_frames);

    for _ in 0..total_frames {
        for led in 0..led_num {
            // Distribute colors across LEDs (zone-based)
            let color_idx = if colors.is_empty() {
                0
            } else {
                led * colors.len() / led_num
            };
            let (r, g, b) = colors.get(color_idx).copied().unwrap_or((0, 0, 0));
            data.push(r);
            data.push(g);
            data.push(b);
        }
    }

    data
}

/// Build a reset packet.
pub fn reset_packet() -> [u8; USB_PACKET_LEN] {
    let mut buf = [0u8; USB_PACKET_LEN];
    buf[0] = CMD_RESET;
    buf
}

/// Format MAC address as colon-separated hex string.
pub fn format_mac(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_mac_packet_format() {
        let pkt = get_mac_packet();
        assert_eq!(pkt[0], CMD_GET_MAC);
        assert_eq!(pkt.len(), USB_PACKET_LEN);
    }

    #[test]
    fn parse_mac_response_valid() {
        let mut data = [0u8; 64];
        data[0] = CMD_GET_MAC;
        data[1] = 0xAA;
        data[2] = 0xBB;
        data[3] = 0xCC;
        data[4] = 0xDD;
        data[5] = 0xEE;
        data[6] = 0xFF;
        let mac = parse_mac_response(&data).unwrap();
        assert_eq!(mac, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn parse_mac_response_empty_mac() {
        let data = [0u8; 64];
        assert!(parse_mac_response(&data).is_none());
    }

    #[test]
    fn query_master_packet_format() {
        let pkt = query_master_packet(DEFAULT_CHANNEL);
        assert_eq!(pkt[0], CMD_QUERY_MASTER);
        assert_eq!(pkt[1], DEFAULT_CHANNEL);
    }

    #[test]
    fn parse_master_response_valid() {
        let mut data = [0u8; 64];
        data[0] = CMD_QUERY_MASTER;
        data[1..7].copy_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        // sys_clock: set time_tmos to 1600 (1600 * 0.625 = 1000)
        data[7] = 0;
        data[8] = 0;
        data[9] = 0x06;
        data[10] = 0x40; // 1600
        data[11] = 0x01;
        data[12] = 0x06; // firmware 0x0106 = 262
        let info = parse_master_response(&data).unwrap();
        assert_eq!(info.mac, [0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        assert_eq!(info.sys_clock, 1000);
        assert_eq!(info.firmware_version, 0x0106);
    }

    #[test]
    fn fragment_rf_packet_produces_4_chunks() {
        let rf = [0xABu8; RF_PACKET_LEN];
        let chunks = fragment_rf_packet(&rf, 8, 1);
        // 240 / 60 = 4 chunks
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0][0], CMD_SEND_RF);
        assert_eq!(chunks[0][1], 0); // frag 0
        assert_eq!(chunks[0][2], 8); // channel
        assert_eq!(chunks[0][3], 1); // rx_type
        assert_eq!(chunks[1][1], 1); // frag 1
        assert_eq!(chunks[2][1], 2); // frag 2
        assert_eq!(chunks[3][1], 3); // frag 3
    }

    #[test]
    fn build_rf_bind_packet_format() {
        let target = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let master = [0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F];
        let pwm = [50, 60, 70, 80];
        let rf = build_rf_bind_packet(&target, &master, 1, 8, 1, &pwm);
        assert_eq!(rf[0], RF_CMD);
        assert_eq!(rf[1], RF_BIND);
        assert_eq!(&rf[2..8], &target);
        assert_eq!(&rf[8..14], &master);
        assert_eq!(rf[14], 1); // rx_type
        assert_eq!(rf[15], 8); // channel
        assert_eq!(rf[16], 1); // slave_index
        assert_eq!(&rf[17..21], &pwm);
    }

    #[test]
    fn build_rf_rgb_sync_packets_format() {
        use crate::protocol::tinyuz;

        let target = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        let master = [0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F];
        let effect_index = [0x01, 0x00, 0x00, 0x00];

        // Generate compressed RGB data (1 fan, static red)
        let rgb_data = render_static_rgb(&[(255, 0, 0)], 1);
        let compressed = tinyuz::tuz_compress(&rgb_data, 4096);

        let packets = build_rf_rgb_sync_packets(
            &target,
            &master,
            &effect_index,
            &compressed,
            STATIC_FRAMES as u16,
            LEDS_PER_FAN as u8,
            60.0,
        );

        // Should have at least metadata + 1 data packet
        assert!(packets.len() >= 2);

        // Check metadata packet (index 0)
        let meta = &packets[0];
        assert_eq!(meta[0], RF_CMD);
        assert_eq!(meta[1], RF_RGB_SYNC);
        assert_eq!(&meta[2..8], &target);
        assert_eq!(&meta[8..14], &master);
        assert_eq!(&meta[14..18], &effect_index);
        assert_eq!(meta[18], 0); // packet_index = 0
        assert_eq!(meta[19], packets.len() as u8); // total_count

        // Check data packet (index 1)
        let data = &packets[1];
        assert_eq!(data[0], RF_CMD);
        assert_eq!(data[1], RF_RGB_SYNC);
        assert_eq!(&data[14..18], &effect_index);
        assert_eq!(data[18], 1); // packet_index = 1
    }

    #[test]
    fn format_mac_string() {
        let mac = [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB];
        assert_eq!(format_mac(&mac), "01:23:45:67:89:ab");
    }

    #[test]
    fn parse_device_list_with_entries() {
        let master_mac = [0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F];
        let mut data = vec![0u8; 4 + DEVICE_ENTRY_LEN * 2];
        data[0] = CMD_SEND_RF;
        data[1] = 1; // 1 device

        let offset = 4;
        // MAC
        data[offset..offset + 6].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        // Master MAC
        data[offset + 6..offset + 12].copy_from_slice(&master_mac);
        // channel, rx_type
        data[offset + 12] = 8;
        data[offset + 13] = 1;
        // dev_type = 0 (SLV3Fan)
        data[offset + 18] = 0;
        // fan_num = 4
        data[offset + 19] = 4;
        // fan_speeds: 1200, 1100, 1000, 900
        data[offset + 28] = 0x04;
        data[offset + 29] = 0xB0; // 1200
        data[offset + 30] = 0x04;
        data[offset + 31] = 0x4C; // 1100
        data[offset + 32] = 0x03;
        data[offset + 33] = 0xE8; // 1000
        data[offset + 34] = 0x03;
        data[offset + 35] = 0x84; // 900
        // fan_pwm
        data[offset + 36] = 80;
        data[offset + 37] = 80;
        data[offset + 38] = 80;
        data[offset + 39] = 80;
        // cmd_seq
        data[offset + 40] = 5;
        // marker
        data[offset + 41] = CMD_GET_MAC;

        let devices = parse_device_list(&data, &master_mac);
        assert_eq!(devices.len(), 1);
        let dev = &devices[0];
        assert_eq!(dev.mac, [0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(dev.fan_num, 4);
        assert_eq!(dev.fan_speeds, [1200, 1100, 1000, 900]);
        assert_eq!(dev.fan_pwm, [80, 80, 80, 80]);
        assert!(dev.bound);
        assert_eq!(dev.dev_type, Slv3DevType::Slv3Fan);
    }
}

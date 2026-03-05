# unifand Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rust crate providing the Lian Li Uni Fan TL USB protocol with device discovery and a CLI tool (`unifanctl`).

**Architecture:** Layered design — protocol (pure data) / transport (USB I/O) / devices (high-level API). Hybrid hidapi + rusb transport.

**Tech Stack:** Rust, hidapi, rusb, des/cbc, clap, thiserror

**Corrected Device IDs (from decompiled KnownDevices.cs):**
- TL Fan Controller: VID=0x0416 (1046), PID=0x7372 (29554), UsagePage=0xFF0B
- TL LCD Wired: VID=0x04FC (1276), PID=0x7393 (29587)
- TL LCD Wireless: VID=0x1CBE (7358), PID=0x0006 (6)
- RF TX Dongle: VID=0x0416, PID=0x8040
- RF RX Dongle: VID=0x0416, PID=0x8041

---

### Task 1: Project Scaffolding

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/bin/unifanctl.rs`
- Create: `src/error.rs`

**Step 1: Initialize cargo project**

Run: `cargo init --name unifand /home/rce/dev/unifand`

Then replace the generated files.

**Step 2: Write Cargo.toml**

```toml
[package]
name = "unifand"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "unifanctl"
path = "src/bin/unifanctl.rs"

[dependencies]
hidapi = "2"
rusb = "0.9"
des = "0.8"
cbc = "0.1"
clap = { version = "4", features = ["derive"] }
thiserror = "2"
```

**Step 3: Write src/error.rs**

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HID error: {0}")]
    Hid(#[from] hidapi::HidError),

    #[error("USB error: {0}")]
    Usb(#[from] rusb::Error),

    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("timeout waiting for device response")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, Error>;
```

**Step 4: Write src/lib.rs stub**

```rust
pub mod error;

pub use error::{Error, Result};
```

**Step 5: Write src/bin/unifanctl.rs stub**

```rust
fn main() {
    println!("unifanctl");
}
```

**Step 6: Verify it compiles**

Run: `cargo build`
Expected: compiles with no errors

**Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/
git commit -m "scaffold unifand crate with error types"
```

---

### Task 2: Protocol — Packet Types (TDD)

**Files:**
- Create: `src/protocol/mod.rs`
- Create: `src/protocol/packet.rs`
- Modify: `src/lib.rs`

**Step 1: Write tests for LedPacket**

In `src/protocol/packet.rs`:

```rust
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test protocol::packet`
Expected: FAIL — `to_bytes`, `from_bytes`, `build_packets` not implemented

**Step 3: Implement LedPacket and LcdPacket**

Add methods to the structs in `src/protocol/packet.rs`:

```rust
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
        buf[2] = (self.data_size >> 24) as u8;
        buf[3] = (self.data_size >> 16) as u8;
        buf[4] = (self.data_size >> 8) as u8;
        buf[5] = self.data_size as u8;
        buf[6] = (self.packet_number >> 16) as u8;
        buf[7] = (self.packet_number >> 8) as u8;
        buf[8] = self.packet_number as u8;
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
        let data_size = (bytes[2] as u32) << 24
            | (bytes[3] as u32) << 16
            | (bytes[4] as u32) << 8
            | bytes[5] as u32;
        let packet_number =
            (bytes[6] as u32) << 16 | (bytes[7] as u32) << 8 | bytes[8] as u32;
        let len = ((bytes[9] as usize) << 8 | bytes[10] as usize)
            .min(bytes.len() - LCD_HEADER_LEN);
        let data = bytes[11..11 + len].to_vec();
        Ok(Self {
            command: bytes[1],
            data_size,
            packet_number,
            data,
        })
    }

    pub fn build_packets(command: u8, data: &[u8]) -> Vec<Self> {
        let total = data.len() as u32;
        data.chunks(LCD_MAX_PAYLOAD)
            .enumerate()
            .map(|(i, chunk)| Self {
                command,
                data_size: total,
                packet_number: i as u32,
                data: chunk.to_vec(),
            })
            .collect()
    }
}
```

**Step 4: Wire up module**

`src/protocol/mod.rs`:
```rust
pub mod packet;
```

Add to `src/lib.rs`:
```rust
pub mod protocol;
```

**Step 5: Run tests to verify they pass**

Run: `cargo test protocol::packet`
Expected: all 5 tests PASS

**Step 6: Commit**

```bash
git add src/protocol/
git commit -m "protocol: LedPacket and LcdPacket with serialization"
```

---

### Task 3: Protocol — Fan and LED Commands (TDD)

**Files:**
- Create: `src/protocol/fan.rs`
- Create: `src/protocol/led.rs`
- Modify: `src/protocol/mod.rs`

**Step 1: Write tests for fan commands**

`src/protocol/fan.rs`:

```rust
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
        .map(|chunk| FanInfo {
            detected: chunk[0] & 0x80 != 0,
            upgrading: chunk[0] & 0x40 != 0,
            port: (chunk[0] >> 4) & 0x03,
            fan_index: chunk[0] & 0x0F,
            rpm: (chunk[1] as u16) << 8 | chunk[2] as u16,
        })
        .collect()
}

pub fn set_fan_speed_packet(port: u8, fan_index: u8, pwm: u8) -> LedPacket {
    LedPacket::new(
        commands::SET_FAN_SPEED,
        vec![(port << 4) | (fan_index & 0x0F), pwm],
    )
}

pub fn set_mb_sync_packet(port: u8, fan_index: u8, sync: bool) -> LedPacket {
    let byte = if sync { 0x80 } else { 0x00 } | (port << 4) | (fan_index & 0x0F);
    LedPacket::new(commands::SET_MB_FAN_SYNC, vec![byte])
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
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[1], 0xA1);
        assert_eq!(bytes[5], 0); // no payload
    }

    #[test]
    fn parse_handshake_response() {
        // Fan on port 1, index 2, detected, 1200 RPM
        // 0x80 | (1 << 4) | 2 = 0x92
        // 1200 = 0x04B0
        let data = vec![0x92, 0x04, 0xB0];
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
        let data = vec![
            0x80, 0x03, 0x20, // port 0, fan 0, detected, 800 RPM
            0x91, 0x05, 0xDC, // port 1, fan 1, detected, 1500 RPM
            0x02, 0x00, 0x00, // port 0, fan 2, NOT detected, 0 RPM
        ];
        let fans = parse_handshake(&data);
        assert_eq!(fans.len(), 3);
        assert!(fans[0].detected);
        assert_eq!(fans[0].rpm, 800);
        assert!(fans[1].detected);
        assert_eq!(fans[1].port, 1);
        assert_eq!(fans[1].rpm, 1500);
        assert!(!fans[2].detected);
    }

    #[test]
    fn set_fan_speed_encoding() {
        let pkt = set_fan_speed_packet(2, 3, 128);
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[1], 0xAA);
        assert_eq!(bytes[5], 2); // 2 bytes payload
        assert_eq!(bytes[6], 0x23); // port 2, fan 3
        assert_eq!(bytes[7], 128);
    }

    #[test]
    fn set_mb_sync_encoding() {
        let pkt = set_mb_sync_packet(1, 0, true);
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[1], 0xB1);
        assert_eq!(bytes[6], 0x90); // sync=1, port=1, fan=0
    }
}
```

**Step 2: Write tests for LED commands**

`src/protocol/led.rs`:

```rust
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
    let mut data = vec![0u8; 20];
    data[0] = (cfg.port << 4) | if cfg.sync { 1 } else { 0 };
    data[1] = (cfg.port << 4) | (cfg.fan_index & 0x0F);
    data[2] = cfg.mode;
    data[3] = cfg.brightness;
    data[4] = cfg.speed;
    for (i, color) in cfg.colors.iter().take(4).enumerate() {
        let offset = 5 + i * 3;
        data[offset] = color.r;
        data[offset + 1] = color.g;
        data[offset + 2] = color.b;
    }
    data[17] = cfg.direction as u8;
    data[18] = if cfg.disabled { 1 } else { 0 };
    data[19] = cfg.colors.len().min(4) as u8;
    LedPacket::new(commands::SET_FAN_LIGHT, data)
}

pub fn set_fan_direction_packet(port: u8, fan_index: u8, swap_top_bottom: bool, swap_left_right: bool) -> LedPacket {
    let flags = if swap_top_bottom { 0x02 } else { 0 }
        | if swap_left_right { 0x01 } else { 0 };
    LedPacket::new(
        commands::SET_FAN_DIRECTION,
        vec![(port << 4) | (fan_index & 0x0F), flags],
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
            mode: 3, // StaticColor
            brightness: 255,
            speed: 0,
            colors: vec![Rgb { r: 0xFF, g: 0x00, b: 0x00 }],
            direction: LightingDirection::RightOrClockwise,
            disabled: false,
        };
        let pkt = set_fan_light_packet(&cfg);
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[1], 0xA3);
        assert_eq!(bytes[5], 20); // payload length
        assert_eq!(bytes[6], 0x00); // port 0, no sync
        assert_eq!(bytes[7], 0x00); // port 0, fan 0
        assert_eq!(bytes[8], 3);    // mode = static
        assert_eq!(bytes[9], 255);  // brightness
        assert_eq!(bytes[11], 0xFF); // red
        assert_eq!(bytes[12], 0x00); // green
        assert_eq!(bytes[13], 0x00); // blue
        assert_eq!(bytes[24], 0);    // not disabled
        assert_eq!(bytes[25], 1);    // 1 color
    }

    #[test]
    fn set_fan_direction_encoding() {
        let pkt = set_fan_direction_packet(1, 2, true, false);
        let bytes = pkt.to_bytes();
        assert_eq!(bytes[1], 0xAE);
        assert_eq!(bytes[6], 0x12); // port 1, fan 2
        assert_eq!(bytes[7], 0x02); // swap top/bottom only
    }
}
```

**Step 3: Run tests to verify they pass**

Run: `cargo test protocol`
Expected: all tests PASS (these are implemented inline)

**Step 4: Wire up modules**

`src/protocol/mod.rs`:
```rust
pub mod fan;
pub mod led;
pub mod packet;
```

**Step 5: Commit**

```bash
git add src/protocol/
git commit -m "protocol: fan speed, LED lighting, and handshake commands"
```

---

### Task 4: Protocol — LCD Commands (TDD)

**Files:**
- Create: `src/protocol/lcd.rs`
- Modify: `src/protocol/mod.rs`

**Step 1: Write LCD command module with tests**

`src/protocol/lcd.rs`:

```rust
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

impl LcdControlSetting {
    pub fn to_bytes(&self) -> Vec<u8> {
        vec![
            self.mode as u8,
            (self.jpg_index >> 8) as u8,
            self.jpg_index as u8,
            0x00, // reserved
            self.brightness,
            self.video_fps,
            self.rotation as u8,
            if self.enable_test { 1 } else { 0 },
            self.test_color.0,
            self.test_color.1,
            self.test_color.2,
        ]
    }
}

pub fn lcd_control_packet(setting: &LcdControlSetting) -> Vec<LcdPacket> {
    LcdPacket::build_packets(commands::LCD_CONTROL, &setting.to_bytes())
}

pub fn handshake_packet() -> Vec<LcdPacket> {
    LcdPacket::build_packets(commands::GET_HANDSHAKE_INFO, &[])
}

pub fn write_jpg_packets(jpg_data: &[u8]) -> Vec<LcdPacket> {
    LcdPacket::build_packets(commands::WRITE_JPG, jpg_data)
}

pub fn write_sync_jpg_packets(jpg_data: &[u8]) -> Vec<LcdPacket> {
    LcdPacket::build_packets(commands::WRITE_SYNC_JPG, jpg_data)
}

pub fn write_avi_packets(h264_data: &[u8]) -> Vec<LcdPacket> {
    LcdPacket::build_packets(commands::WRITE_AVI, h264_data)
}

#[derive(Debug)]
pub struct HandshakeInfo {
    pub mode: u8,
    pub frame_index: u16,
}

pub fn parse_handshake(data: &[u8]) -> crate::Result<HandshakeInfo> {
    if data.len() < 3 {
        return Err(crate::Error::InvalidResponse(
            "handshake response too short".into(),
        ));
    }
    Ok(HandshakeInfo {
        mode: data[0],
        frame_index: (data[1] as u16) << 8 | data[2] as u16,
    })
}

#[derive(Debug)]
pub struct SerialNumberInfo {
    pub serial: String,
    pub port: u8,
    pub lcd_index: u8,
}

pub fn parse_serial_number(data: &[u8]) -> crate::Result<SerialNumberInfo> {
    if data.len() < 34 {
        return Err(crate::Error::InvalidResponse(
            "serial number response too short".into(),
        ));
    }
    let serial = String::from_utf8_lossy(&data[..32]).trim_end_matches('\0').to_string();
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
        assert_eq!(bytes[4], 80); // brightness
        assert_eq!(bytes[5], 30); // fps
        assert_eq!(bytes[6], 2);  // 180 degrees
    }

    #[test]
    fn write_jpg_creates_correct_packets() {
        let jpg_data = vec![0xFF; 1000];
        let packets = write_jpg_packets(&jpg_data);
        assert_eq!(packets.len(), 2); // 501 + 499
        assert_eq!(packets[0].command, 0x41);
        assert_eq!(packets[0].data_size, 1000);
        assert_eq!(packets[0].data.len(), 501);
        assert_eq!(packets[1].data.len(), 499);
    }

    #[test]
    fn parse_handshake_info() {
        let data = vec![0x01, 0x00, 0x0A]; // mode=1 (ShowJPG), frame=10
        let info = parse_handshake(&data).unwrap();
        assert_eq!(info.mode, 1);
        assert_eq!(info.frame_index, 10);
    }

    #[test]
    fn parse_serial_number_info() {
        let mut data = vec![0u8; 34];
        data[..5].copy_from_slice(b"SN123");
        data[32] = 2; // port
        data[33] = 1; // lcd_index
        let info = parse_serial_number(&data).unwrap();
        assert_eq!(info.serial, "SN123");
        assert_eq!(info.port, 2);
        assert_eq!(info.lcd_index, 1);
    }
}
```

**Step 2: Run tests**

Run: `cargo test protocol::lcd`
Expected: all 4 tests PASS

**Step 3: Add to mod.rs**

In `src/protocol/mod.rs` add: `pub mod lcd;`

**Step 4: Commit**

```bash
git add src/protocol/
git commit -m "protocol: LCD control, JPEG/AVI transfer, handshake parsing"
```

---

### Task 5: Transport — HID Layer

**Files:**
- Create: `src/transport/mod.rs`
- Create: `src/transport/hid.rs`
- Modify: `src/lib.rs`

**Step 1: Write HID transport wrapper**

`src/transport/hid.rs`:

```rust
use hidapi::{HidApi, HidDevice};

use crate::error::Result;
use crate::protocol::packet::{LED_PACKET_LEN, LCD_OUTPUT_LEN, LCD_INPUT_LEN};

pub struct HidTransport {
    device: HidDevice,
}

impl HidTransport {
    pub fn open(api: &HidApi, vid: u16, pid: u16) -> Result<Self> {
        let device = api.open(vid, pid)?;
        Ok(Self { device })
    }

    pub fn open_path(api: &HidApi, path: &str) -> Result<Self> {
        let path = std::ffi::CString::new(path)
            .map_err(|_| crate::Error::DeviceNotFound("invalid path".into()))?;
        let device = api.open_path(&path)?;
        Ok(Self { device })
    }

    /// Send a 64-byte LED/fan packet and read 64-byte response.
    pub fn led_write_read(&self, data: &[u8; LED_PACKET_LEN]) -> Result<[u8; LED_PACKET_LEN]> {
        self.device.write(data)?;
        let mut buf = [0u8; LED_PACKET_LEN];
        let n = self.device.read_timeout(&mut buf, 100)?;
        if n == 0 {
            return Err(crate::Error::Timeout);
        }
        Ok(buf)
    }

    /// Send a 64-byte LED/fan packet without reading response.
    pub fn led_write(&self, data: &[u8; LED_PACKET_LEN]) -> Result<()> {
        self.device.write(data)?;
        Ok(())
    }

    /// Send a 512-byte LCD packet and read 64-byte response.
    pub fn lcd_write_read(&self, data: &[u8; LCD_OUTPUT_LEN]) -> Result<[u8; LCD_INPUT_LEN]> {
        self.device.write(data)?;
        let mut buf = [0u8; LCD_INPUT_LEN];
        let n = self.device.read_timeout(&mut buf, 200)?;
        if n == 0 {
            return Err(crate::Error::Timeout);
        }
        Ok(buf)
    }

    /// Send a 512-byte LCD packet without reading response (for sync JPG).
    pub fn lcd_write(&self, data: &[u8; LCD_OUTPUT_LEN]) -> Result<()> {
        self.device.write(data)?;
        Ok(())
    }
}
```

`src/transport/mod.rs`:
```rust
pub mod hid;
```

Add to `src/lib.rs`: `pub mod transport;`

**Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles (no unit tests for I/O layer — tested via integration)

**Step 3: Commit**

```bash
git add src/transport/
git commit -m "transport: HID layer wrapping hidapi for LED and LCD packets"
```

---

### Task 6: Transport — USB Bulk + DES Encryption (Wireless)

**Files:**
- Create: `src/transport/usb.rs`
- Modify: `src/transport/mod.rs`

**Step 1: Write DES encryption with tests**

`src/transport/usb.rs`:

```rust
use cbc::cipher::{BlockEncryptMut, BlockDecryptMut, KeyIvInit};

type DesCbcEnc = cbc::Encryptor<des::Des>;
type DesCbcDec = cbc::Decryptor<des::Des>;

const DES_KEY: &[u8; 8] = b"slv3tuzx";
const CMD_MAGIC: [u8; 2] = [0x1A, 0x6D];
const CMD_BUF_LEN: usize = 504;

pub fn encrypt(plaintext: &[u8]) -> Vec<u8> {
    let mut buf = plaintext.to_vec();
    // Pad to multiple of 8
    while buf.len() % 8 != 0 {
        buf.push(0);
    }
    let enc = DesCbcEnc::new(DES_KEY.into(), DES_KEY.into());
    enc.encrypt_padded_mut::<cbc::cipher::block_padding::NoPadding>(&mut buf, buf.len())
        .expect("encryption failed");
    buf
}

pub fn decrypt(ciphertext: &[u8]) -> Vec<u8> {
    let mut buf = ciphertext.to_vec();
    let dec = DesCbcDec::new(DES_KEY.into(), DES_KEY.into());
    dec.decrypt_padded_mut::<cbc::cipher::block_padding::NoPadding>(&mut buf)
        .expect("decryption failed")
        .to_vec()
}

pub fn build_command_buffer(cmd: u8, data: Option<&[u8]>) -> [u8; CMD_BUF_LEN] {
    let mut buf = [0u8; CMD_BUF_LEN];
    buf[0] = cmd;
    buf[2] = CMD_MAGIC[0];
    buf[3] = CMD_MAGIC[1];
    // Timestamp at [4..8] — use current time
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;
    buf[4..8].copy_from_slice(&ts.to_le_bytes());
    if let Some(data) = data {
        let len = data.len() as u32;
        buf[8] = (len >> 24) as u8;
        buf[9] = (len >> 16) as u8;
        buf[10] = (len >> 8) as u8;
        buf[11] = len as u8;
        let copy_len = data.len().min(CMD_BUF_LEN - 12);
        buf[12..12 + copy_len].copy_from_slice(&data[..copy_len]);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let original = b"hello world! this is a test msg."; // 31 bytes
        let encrypted = encrypt(original);
        assert_ne!(&encrypted[..original.len()], original);
        let decrypted = decrypt(&encrypted);
        assert_eq!(&decrypted[..original.len()], original);
    }

    #[test]
    fn command_buffer_structure() {
        let buf = build_command_buffer(0x0A, Some(&[0x01, 0x02]));
        assert_eq!(buf[0], 0x0A); // command
        assert_eq!(buf[2], 0x1A); // magic
        assert_eq!(buf[3], 0x6D); // magic
        // data size = 2 at bytes [8..12], big-endian
        assert_eq!(buf[8], 0);
        assert_eq!(buf[9], 0);
        assert_eq!(buf[10], 0);
        assert_eq!(buf[11], 2);
        assert_eq!(buf[12], 0x01);
        assert_eq!(buf[13], 0x02);
    }

    #[test]
    fn command_buffer_no_data() {
        let buf = build_command_buffer(0x0A, None);
        assert_eq!(buf[0], 0x0A);
        assert_eq!(buf[8], 0); // no data size
    }
}
```

**Step 2: Run tests**

Run: `cargo test transport::usb`
Expected: all 3 tests PASS

Note: The `encrypt_padded_mut` / `decrypt_padded_mut` API may need adjustment based on exact `des` and `cbc` crate versions. If the API doesn't match, use `BlockEncryptMut::encrypt_blocks_mut` on manually padded 8-byte blocks. Adjust the implementation to whatever compiles with the versions pulled by Cargo.

**Step 3: Add to mod.rs**

In `src/transport/mod.rs` add: `pub mod usb;`

**Step 4: Commit**

```bash
git add src/transport/
git commit -m "transport: DES-CBC encryption for wireless TL LCD protocol"
```

---

### Task 7: Device Discovery

**Files:**
- Create: `src/device.rs`
- Modify: `src/lib.rs`

**Step 1: Write device discovery module**

`src/device.rs`:

```rust
use hidapi::HidApi;

use crate::Result;

pub mod known {
    pub const TL_FAN_VID: u16 = 0x0416;
    pub const TL_FAN_PID: u16 = 0x7372;
    pub const TL_FAN_USAGE_PAGE: u16 = 0xFF0B;

    pub const TL_LCD_WIRED_VID: u16 = 0x04FC;
    pub const TL_LCD_WIRED_PID: u16 = 0x7393;

    pub const TL_LCD_WIRELESS_VID: u16 = 0x1CBE;
    pub const TL_LCD_WIRELESS_PID: u16 = 0x0006;

    pub const RF_TX_VID: u16 = 0x0416;
    pub const RF_TX_PID: u16 = 0x8040;

    pub const RF_RX_VID: u16 = 0x0416;
    pub const RF_RX_PID: u16 = 0x8041;
}

#[derive(Debug, Clone)]
pub enum DeviceKind {
    TlFanController,
    TlLcdWired,
    TlLcdWireless,
    RfTxDongle,
    RfRxDongle,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub kind: DeviceKind,
    pub path: String,
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
}

impl std::fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TlFanController => write!(f, "TL Fan Controller"),
            Self::TlLcdWired => write!(f, "TL LCD (Wired)"),
            Self::TlLcdWireless => write!(f, "TL LCD (Wireless)"),
            Self::RfTxDongle => write!(f, "RF TX Dongle"),
            Self::RfRxDongle => write!(f, "RF RX Dongle"),
        }
    }
}

impl std::fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{:04x}:{:04x}] {}",
            self.kind, self.vid, self.pid, self.path
        )
    }
}

pub fn discover() -> Result<Vec<DeviceInfo>> {
    let api = HidApi::new()?;
    let mut devices = Vec::new();

    let known_devices = [
        (known::TL_FAN_VID, known::TL_FAN_PID, DeviceKind::TlFanController),
        (known::TL_LCD_WIRED_VID, known::TL_LCD_WIRED_PID, DeviceKind::TlLcdWired),
        (known::TL_LCD_WIRELESS_VID, known::TL_LCD_WIRELESS_PID, DeviceKind::TlLcdWireless),
        (known::RF_TX_VID, known::RF_TX_PID, DeviceKind::RfTxDongle),
        (known::RF_RX_VID, known::RF_RX_PID, DeviceKind::RfRxDongle),
    ];

    for info in api.device_list() {
        for (vid, pid, kind) in &known_devices {
            if info.vendor_id() == *vid && info.product_id() == *pid {
                // For TL Fan, filter by usage page
                if matches!(kind, DeviceKind::TlFanController)
                    && info.usage_page() != known::TL_FAN_USAGE_PAGE
                {
                    continue;
                }
                devices.push(DeviceInfo {
                    kind: kind.clone(),
                    path: info.path().to_string_lossy().into(),
                    vid: info.vendor_id(),
                    pid: info.product_id(),
                    serial: info
                        .serial_number()
                        .map(|s| s.to_string()),
                });
            }
        }
    }

    Ok(devices)
}
```

**Step 2: Add to lib.rs**

```rust
pub mod device;
pub mod error;
pub mod protocol;
pub mod transport;

pub use device::{DeviceInfo, DeviceKind, discover};
pub use error::{Error, Result};
```

**Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles

**Step 4: Commit**

```bash
git add src/device.rs src/lib.rs
git commit -m "device discovery: enumerate Lian Li fans by VID/PID"
```

---

### Task 8: Device Implementations — TL Fan Controller

**Files:**
- Create: `src/devices/mod.rs`
- Create: `src/devices/tl_fan.rs`
- Modify: `src/lib.rs`

**Step 1: Write TL fan controller device**

`src/devices/tl_fan.rs`:

```rust
use hidapi::HidApi;

use crate::device::{known, DeviceInfo};
use crate::protocol::{fan, packet::LedPacket};
use crate::transport::hid::HidTransport;
use crate::Result;

pub struct TlFanController {
    transport: HidTransport,
}

impl TlFanController {
    pub fn open(api: &HidApi, info: &DeviceInfo) -> Result<Self> {
        let transport = HidTransport::open_path(api, &info.path)?;
        Ok(Self { transport })
    }

    pub fn open_first(api: &HidApi) -> Result<Self> {
        let transport = HidTransport::open(api, known::TL_FAN_VID, known::TL_FAN_PID)?;
        Ok(Self { transport })
    }

    pub fn handshake(&self) -> Result<Vec<fan::FanInfo>> {
        let pkt = fan::handshake_packet();
        let resp = self.transport.led_write_read(&pkt.to_bytes())?;
        let resp = LedPacket::from_bytes(&resp)?;
        Ok(fan::parse_handshake(&resp.data))
    }

    pub fn set_fan_speed(&self, port: u8, fan_index: u8, pwm: u8) -> Result<()> {
        let pkt = fan::set_fan_speed_packet(port, fan_index, pwm);
        self.transport.led_write_read(&pkt.to_bytes())?;
        Ok(())
    }

    pub fn set_mb_sync(&self, port: u8, fan_index: u8, sync: bool) -> Result<()> {
        let pkt = fan::set_mb_sync_packet(port, fan_index, sync);
        self.transport.led_write_read(&pkt.to_bytes())?;
        Ok(())
    }

    pub fn blink_port(&self, port: u8) -> Result<()> {
        let pkt = fan::blink_port_packet(port);
        self.transport.led_write_read(&pkt.to_bytes())?;
        Ok(())
    }

    pub fn set_fan_light(&self, cfg: &crate::protocol::led::FanLightConfig) -> Result<()> {
        let pkt = crate::protocol::led::set_fan_light_packet(cfg);
        self.transport.led_write_read(&pkt.to_bytes())?;
        Ok(())
    }
}
```

`src/devices/mod.rs`:
```rust
pub mod tl_fan;
```

Add `pub mod devices;` to `src/lib.rs`.

**Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles

**Step 3: Commit**

```bash
git add src/devices/
git commit -m "devices: TL fan controller with speed, sync, and lighting"
```

---

### Task 9: Device Implementations — TL LCD (Wired + Wireless)

**Files:**
- Create: `src/devices/tl_lcd_wired.rs`
- Create: `src/devices/tl_lcd_wireless.rs`
- Modify: `src/devices/mod.rs`

**Step 1: Write wired LCD device**

`src/devices/tl_lcd_wired.rs`:

```rust
use hidapi::HidApi;

use crate::device::DeviceInfo;
use crate::protocol::{lcd, packet::LcdPacket};
use crate::transport::hid::HidTransport;
use crate::Result;

pub struct TlLcdWired {
    transport: HidTransport,
}

impl TlLcdWired {
    pub fn open(api: &HidApi, info: &DeviceInfo) -> Result<Self> {
        let transport = HidTransport::open_path(api, &info.path)?;
        Ok(Self { transport })
    }

    pub fn handshake(&self) -> Result<lcd::HandshakeInfo> {
        let packets = lcd::handshake_packet();
        // Handshake produces a single empty-data packet
        let pkt = &packets[0];
        let resp = self.transport.lcd_write_read(&pkt.to_bytes())?;
        let resp = LcdPacket::from_bytes(&resp)?;
        lcd::parse_handshake(&resp.data)
    }

    pub fn set_control(&self, setting: &lcd::LcdControlSetting) -> Result<()> {
        let packets = lcd::lcd_control_packet(setting);
        for pkt in &packets {
            self.transport.lcd_write_read(&pkt.to_bytes())?;
        }
        Ok(())
    }

    pub fn send_jpg(&self, jpg_data: &[u8]) -> Result<()> {
        let packets = lcd::write_jpg_packets(jpg_data);
        for pkt in &packets {
            self.transport.lcd_write_read(&pkt.to_bytes())?;
        }
        Ok(())
    }

    pub fn send_sync_jpg(&self, jpg_data: &[u8]) -> Result<()> {
        let packets = lcd::write_sync_jpg_packets(jpg_data);
        for pkt in &packets {
            // Sync JPG is fire-and-forget
            self.transport.lcd_write(&pkt.to_bytes())?;
        }
        Ok(())
    }

    pub fn send_avi(&self, h264_data: &[u8]) -> Result<()> {
        let packets = lcd::write_avi_packets(h264_data);
        for pkt in &packets {
            self.transport.lcd_write_read(&pkt.to_bytes())?;
        }
        Ok(())
    }
}
```

**Step 2: Write wireless LCD device stub**

`src/devices/tl_lcd_wireless.rs`:

```rust
use hidapi::HidApi;

use crate::device::DeviceInfo;
use crate::protocol::lcd;
use crate::transport::hid::HidTransport;
use crate::Result;

/// Wireless TL LCD (1CBE:0006).
///
/// HID reports work for control commands (handshake, lcd control).
/// Video/image streaming uses USB bulk with DES encryption — that path
/// is not yet implemented (needs rusb bulk endpoint access).
pub struct TlLcdWireless {
    transport: HidTransport,
}

impl TlLcdWireless {
    pub fn open(api: &HidApi, info: &DeviceInfo) -> Result<Self> {
        let transport = HidTransport::open_path(api, &info.path)?;
        Ok(Self { transport })
    }

    pub fn handshake(&self) -> Result<lcd::HandshakeInfo> {
        let packets = lcd::handshake_packet();
        let pkt = &packets[0];
        let resp = self.transport.lcd_write_read(&pkt.to_bytes())?;
        let resp = crate::protocol::packet::LcdPacket::from_bytes(&resp)?;
        lcd::parse_handshake(&resp.data)
    }

    pub fn set_control(&self, setting: &lcd::LcdControlSetting) -> Result<()> {
        let packets = lcd::lcd_control_packet(setting);
        for pkt in &packets {
            self.transport.lcd_write_read(&pkt.to_bytes())?;
        }
        Ok(())
    }

    // TODO: send_jpg, send_avi via USB bulk + DES encryption
    // Requires rusb bulk endpoint — will be added when we have
    // the wireless hardware connected for testing.
}
```

**Step 3: Update devices/mod.rs**

```rust
pub mod tl_fan;
pub mod tl_lcd_wired;
pub mod tl_lcd_wireless;
```

**Step 4: Verify it compiles**

Run: `cargo build`
Expected: compiles

**Step 5: Commit**

```bash
git add src/devices/
git commit -m "devices: wired and wireless TL LCD with control and image transfer"
```

---

### Task 10: CLI Binary (`unifanctl`)

**Files:**
- Modify: `src/bin/unifanctl.rs`

**Step 1: Write the CLI**

`src/bin/unifanctl.rs`:

```rust
use clap::{Parser, Subcommand};
use unifand::{device::DeviceKind, devices};

#[derive(Parser)]
#[command(name = "unifanctl", about = "Control Lian Li Uni Fans")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all detected Lian Li devices
    Discover,
    /// Fan control commands
    Fan {
        #[command(subcommand)]
        command: FanCommands,
    },
    /// LCD display commands
    Lcd {
        #[command(subcommand)]
        command: LcdCommands,
    },
}

#[derive(Subcommand)]
enum FanCommands {
    /// Show detected fans and RPM
    Status,
    /// Set fan speed
    SetSpeed {
        /// Port number (0-3)
        port: u8,
        /// Fan index on port (0-15)
        fan: u8,
        /// PWM value (0-255)
        pwm: u8,
    },
    /// Blink a port's LEDs for identification
    Blink {
        /// Port number (0-3)
        port: u8,
    },
}

#[derive(Subcommand)]
enum LcdCommands {
    /// Set LCD brightness
    Brightness {
        /// Brightness level (0-100)
        level: u8,
    },
    /// Rotate LCD display
    Rotate {
        /// Rotation in degrees (0, 90, 180, 270)
        degrees: u16,
    },
    /// Display a JPEG image
    ShowImage {
        /// Path to JPEG file
        path: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let api = hidapi::HidApi::new()?;

    match cli.command {
        Commands::Discover => {
            let devices = unifand::discover()?;
            if devices.is_empty() {
                println!("No Lian Li devices found.");
                println!("Check udev rules and device connections.");
            } else {
                for dev in &devices {
                    println!("{dev}");
                }
            }
        }
        Commands::Fan { command } => {
            let fan_ctrl = devices::tl_fan::TlFanController::open_first(&api)?;
            match command {
                FanCommands::Status => {
                    let fans = fan_ctrl.handshake()?;
                    if fans.is_empty() {
                        println!("No fans detected.");
                    }
                    for fan in &fans {
                        let status = if fan.detected { "detected" } else { "absent" };
                        println!(
                            "Port {} Fan {:2}: {:>8} {:>5} RPM",
                            fan.port, fan.fan_index, status, fan.rpm
                        );
                    }
                }
                FanCommands::SetSpeed { port, fan, pwm } => {
                    fan_ctrl.set_fan_speed(port, fan, pwm)?;
                    println!("Set port {port} fan {fan} to PWM {pwm}");
                }
                FanCommands::Blink { port } => {
                    fan_ctrl.blink_port(port)?;
                    println!("Blinking port {port}");
                }
            }
        }
        Commands::Lcd { command } => {
            use unifand::protocol::lcd::{LcdControlSetting, LcdMode, ScreenRotation};

            // Try wireless first, then wired
            let device_infos = unifand::discover()?;
            let lcd_info = device_infos
                .iter()
                .find(|d| matches!(d.kind, DeviceKind::TlLcdWireless | DeviceKind::TlLcdWired))
                .ok_or(unifand::Error::DeviceNotFound("no LCD device found".into()))?;

            match command {
                LcdCommands::Brightness { level } => {
                    match lcd_info.kind {
                        DeviceKind::TlLcdWired => {
                            let lcd = devices::tl_lcd_wired::TlLcdWired::open(&api, lcd_info)?;
                            lcd.set_control(&LcdControlSetting {
                                mode: LcdMode::LcdSetting,
                                jpg_index: 0,
                                brightness: level,
                                video_fps: 30,
                                rotation: ScreenRotation::Deg0,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                        }
                        DeviceKind::TlLcdWireless => {
                            let lcd = devices::tl_lcd_wireless::TlLcdWireless::open(&api, lcd_info)?;
                            lcd.set_control(&LcdControlSetting {
                                mode: LcdMode::LcdSetting,
                                jpg_index: 0,
                                brightness: level,
                                video_fps: 30,
                                rotation: ScreenRotation::Deg0,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                        }
                        _ => unreachable!(),
                    }
                    println!("Set brightness to {level}");
                }
                LcdCommands::Rotate { degrees } => {
                    let rotation = match degrees {
                        0 => ScreenRotation::Deg0,
                        90 => ScreenRotation::Deg90,
                        180 => ScreenRotation::Deg180,
                        270 => ScreenRotation::Deg270,
                        _ => {
                            eprintln!("Invalid rotation: {degrees}. Use 0, 90, 180, or 270.");
                            std::process::exit(1);
                        }
                    };
                    match lcd_info.kind {
                        DeviceKind::TlLcdWired => {
                            let lcd = devices::tl_lcd_wired::TlLcdWired::open(&api, lcd_info)?;
                            lcd.set_control(&LcdControlSetting {
                                mode: LcdMode::LcdSetting,
                                jpg_index: 0,
                                brightness: 80,
                                video_fps: 30,
                                rotation,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                        }
                        DeviceKind::TlLcdWireless => {
                            let lcd = devices::tl_lcd_wireless::TlLcdWireless::open(&api, lcd_info)?;
                            lcd.set_control(&LcdControlSetting {
                                mode: LcdMode::LcdSetting,
                                jpg_index: 0,
                                brightness: 80,
                                video_fps: 30,
                                rotation,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                        }
                        _ => unreachable!(),
                    }
                    println!("Rotated display to {degrees} degrees");
                }
                LcdCommands::ShowImage { path } => {
                    let jpg_data = std::fs::read(&path)?;
                    match lcd_info.kind {
                        DeviceKind::TlLcdWired => {
                            let lcd = devices::tl_lcd_wired::TlLcdWired::open(&api, lcd_info)?;
                            lcd.send_jpg(&jpg_data)?;
                        }
                        DeviceKind::TlLcdWireless => {
                            eprintln!("Wireless LCD image push not yet implemented (needs USB bulk).");
                            std::process::exit(1);
                        }
                        _ => unreachable!(),
                    }
                    println!("Sent {path} to LCD");
                }
            }
        }
    }

    Ok(())
}
```

Note: Add `anyhow = "1"` to `[dependencies]` in `Cargo.toml` for the CLI error handling.

**Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles

**Step 3: Test help output**

Run: `cargo run --bin unifanctl -- --help`
Expected: shows usage with discover, fan, lcd subcommands

**Step 4: Commit**

```bash
git add src/bin/unifanctl.rs Cargo.toml
git commit -m "unifanctl CLI: discover, fan control, and LCD commands"
```

---

### Task 11: Update udev Rules

**Files:**
- Modify: `udev/60-unifand.rules`

**Step 1: Update rules file with all known devices**

```
# Lian Li Uni Fan - unifand
# TL Fan Controller (wired)
SUBSYSTEMS=="usb", ATTRS{idVendor}=="0416", ATTRS{idProduct}=="7372", MODE="0660", TAG+="uaccess"
# TL LCD Wired
SUBSYSTEMS=="usb", ATTRS{idVendor}=="04fc", ATTRS{idProduct}=="7393", MODE="0660", TAG+="uaccess"
# TL LCD Wireless
SUBSYSTEMS=="usb", ATTRS{idVendor}=="1cbe", ATTRS{idProduct}=="0006", MODE="0660", TAG+="uaccess"
# Wireless RF TX Dongle
SUBSYSTEMS=="usb", ATTRS{idVendor}=="0416", ATTRS{idProduct}=="8040", MODE="0660", TAG+="uaccess"
# Wireless RF RX Dongle
SUBSYSTEMS=="usb", ATTRS{idVendor}=="0416", ATTRS{idProduct}=="8041", MODE="0660", TAG+="uaccess"
```

**Step 2: Commit**

```bash
git add udev/60-unifand.rules
git commit -m "udev: add rules for all known TL fan/LCD devices"
```

---

### Task 12: Integration Test — Discovery on Real Hardware

**Step 1: Reload udev rules**

Run: `sudo udevadm control --reload-rules && sudo udevadm trigger`

**Step 2: Run discovery**

Run: `cargo run --bin unifanctl -- discover`
Expected: lists at least the TL LCD Wireless (1CBE:0006) device if connected

**Step 3: If device found, test handshake**

Run: `cargo run --bin unifanctl -- fan status` (if fan controller present)

**Step 4: Fix any issues found during real hardware testing**

**Step 5: Commit any fixes**

---

### Task 13: run.sh Script

**Files:**
- Create: `run.sh`

**Step 1: Write run script per project conventions**

```bash
#!/usr/bin/env bash
set -euo pipefail

# Check for Rust toolchain
if ! command -v cargo &> /dev/null; then
    echo "Error: Rust toolchain not found. Install via: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# Check for system dependencies
for lib in hidapi libusb; do
    if ! pkg-config --exists $lib 2>/dev/null; then
        echo "Error: $lib not found. Install via: sudo pacman -S hidapi libusb"
        exit 1
    fi
done

cargo build --release
echo "Built successfully. Run with:"
echo "  ./target/release/unifanctl discover"
echo "  ./target/release/unifanctl fan status"
echo "  ./target/release/unifanctl --help"
```

**Step 2: Make executable and commit**

```bash
chmod +x run.sh
git add run.sh
git commit -m "add run.sh build script"
```

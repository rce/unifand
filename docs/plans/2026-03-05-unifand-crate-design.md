# unifand Crate Design

Linux Rust crate for controlling Lian Li Uni Fan TL series (wired and wireless).
Replaces L-Connect 3 with a native library and CLI tool.

## Hardware Support

| Device | VID:PID | Transport | Purpose |
|--------|---------|-----------|---------|
| TL Fan Controller | 0416:7372 | HID (report 0x01, 64B) | Fan speed, RGB, grouping |
| TL LCD Wired | 1CBE:A055 | HID (report 0x02, 512B) | LCD display control |
| TL LCD Wireless | 1CBE:0006 | HID + USB bulk | LCD display (encrypted) |
| RF TX Dongle | 0416:8040 | USB bulk | Wireless fan TX |
| RF RX Dongle | 0416:8041 | USB bulk | Wireless fan RX |

Wireless hardware available for testing. Wired support based on decompiled protocol, untestable locally.

## Architecture

Hybrid transport: `hidapi` for HID reports, `rusb` for USB bulk transfers.

```
src/
├── lib.rs                    # public API: discover(), open_fan_controller(), etc.
├── error.rs                  # thiserror error types
├── device.rs                 # discovery & enumeration
├── protocol/
│   ├── mod.rs
│   ├── packet.rs             # LedPacket (64B) and LcdPacket (512B) builders
│   ├── fan.rs                # fan speed commands
│   ├── led.rs                # RGB lighting commands
│   └── lcd.rs                # LCD control + image/video transfer
├── transport/
│   ├── mod.rs
│   ├── hid.rs                # hidapi wrapper
│   └── usb.rs                # rusb wrapper + DES-CBC encryption
├── devices/
│   ├── mod.rs
│   ├── tl_fan.rs             # TL fan controller (0416:7372)
│   ├── tl_lcd_wired.rs       # Wired TL LCD (1CBE:A055)
│   └── tl_lcd_wireless.rs    # Wireless TL LCD (1CBE:0006)
└── bin/
    └── unifanctl.rs           # CLI binary
```

### Layer Separation

- **protocol/** — pure data, no I/O. Builds/parses byte arrays matching wire format.
- **transport/** — USB I/O via hidapi and rusb. Handles encryption for wireless.
- **devices/** — combines protocol + transport into device-specific high-level API.

## Protocol

### Packet Formats

**LedPacket (64 bytes)** — fan and RGB commands:
```
[0]    Report ID (0x01)
[1]    Command
[2]    Reserved
[3-4]  Packet number (big-endian)
[5]    Payload length (max 58)
[6-63] Payload
```

**LcdPacket (512 bytes)** — LCD commands:
```
[0]      Report ID (0x02)
[1]      Command
[2-5]    Total data size (big-endian)
[6-8]    Packet number (big-endian, 24-bit)
[9-10]   Payload length (big-endian, max 501)
[11-511] Payload
```

### Fan Commands

| Command | Byte | Payload |
|---------|------|---------|
| Handshake | 0xA1 | None. Response: 3 bytes per fan (flags+port/idx, RPM high, RPM low) |
| SetFanSpeed | 0xAA | [port<<4\|fan_idx, pwm(0-255)] |
| SetMBSync | 0xB1 | [sync<<7\|port<<4\|fan_idx] |
| GetFirmwareVersion | 0xA6 | [is_slave, port/fan_idx] |

### LED Commands

| Command | Byte | Payload |
|---------|------|---------|
| SetFanLight | 0xA3 | port/sync flags, port/fan, mode, brightness, speed, 4x RGB, direction, disabled |
| SetGroupLight | 0xB0 | group, mode, brightness, speed, 4x RGB, disabled |
| SetFanGroup | 0xAD | group_id, fan_count, fan descriptors |
| SetFanDirection | 0xAE | port/fan, swap flags |
| BlinkPort | 0xB4 | port(0-3) |
| TestLight | 0xB3 | type(1-3), target, is_bottom |

### LCD Commands

| Command | Byte | Payload |
|---------|------|---------|
| LCDControl | 0x40 | mode, jpg_index(2B), reserved, brightness, fps, rotation, test_enable, RGB |
| WriteJPG | 0x41 | Multi-packet JPEG data (chunked at 501B) |
| WriteAVI | 0x45 | H.264 video data |
| WriteSyncJPG | 0x46 | Real-time JPEG frame |
| WriteBootJPG | 0x48 | Startup image |
| WriteBootAVI | 0x47 | Startup video |
| GetHandshakeInfo | 0x3C | Response: mode, frame_index(2B) |
| GetProductInfo | 0x3D | Response: 2 packets (version string, datetime string) |

### Wireless Encryption

DES-CBC with key and IV both `"slv3tuzx"` (8 bytes). Applied to 504-byte command buffers:
```
[0]    Command type
[1]    Reserved
[2-3]  Magic: 0x1A, 0x6D
[4-7]  Timestamp (little-endian)
[8-11] Data size (big-endian, if data present)
[12+]  Payload
```

### LCD Specs

- Resolution: 400x400 pixels, 24-bit RGB
- Brightness: 0-100
- Rotation: 0/90/180/270 degrees
- Max 3 LCD fans per port, 4 ports per controller
- JPEG quality: 95% recommended, max ~1MB
- Video: H.264, 400x400, 25-30 FPS, no B-frames

## CLI (`unifanctl`)

```
unifanctl discover                    # list all detected devices
unifanctl fan get-status              # handshake: detected fans + RPM
unifanctl fan set-speed 0 0 128       # port 0, fan 0, PWM 128
unifanctl lcd brightness 80           # set brightness
unifanctl lcd rotate 180              # rotate display
unifanctl lcd show-image foo.jpg      # push JPEG
unifanctl led set 0 0 static --color FF0000 --brightness 255
```

## Dependencies

- `hidapi` — HID device communication
- `rusb` — USB bulk transfers
- `des`, `cbc` — DES-CBC encryption for wireless
- `clap` (derive) — CLI argument parsing
- `thiserror` — error types

## udev Rules

Required for unprivileged access. All known devices:

```
# Lian Li TL Fan Controller (wired)
SUBSYSTEMS=="usb", ATTRS{idVendor}=="0416", ATTRS{idProduct}=="7372", MODE="0660", TAG+="uaccess"
# Lian Li TL LCD Wired
SUBSYSTEMS=="usb", ATTRS{idVendor}=="1cbe", ATTRS{idProduct}=="a055", MODE="0660", TAG+="uaccess"
# Lian Li TL LCD Wireless
SUBSYSTEMS=="usb", ATTRS{idVendor}=="1cbe", ATTRS{idProduct}=="0006", MODE="0660", TAG+="uaccess"
# Lian Li Wireless RF TX Dongle
SUBSYSTEMS=="usb", ATTRS{idVendor}=="0416", ATTRS{idProduct}=="8040", MODE="0660", TAG+="uaccess"
# Lian Li Wireless RF RX Dongle
SUBSYSTEMS=="usb", ATTRS{idVendor}=="0416", ATTRS{idProduct}=="8041", MODE="0660", TAG+="uaccess"
```

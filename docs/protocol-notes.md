# Lian Li L-Connect 3 Protocol Notes

Reverse-engineered from decompiled C# source of L-Connect 3 for Windows.

## Device Inventory

| Device | VID:PID | Transport | Role |
|--------|---------|-----------|------|
| TL Fan Controller | 0416:7372 | HID (Report ID 0x01) | Wired fan speed/LED control |
| TL LCD Wired | (per HID enumeration) | HID (Report ID 0x02) | Wired LCD panel on fan hub |
| SLV3H Hub | 1A86:2107 | HID (Report ID 0x00) | Wireless hub MAC lookup |
| RF TX Dongle | 0416:8040 | USB Bulk (EP 0x01/0x81) | Send commands to wireless devices |
| RF RX Dongle | 0416:8041 | USB Bulk (EP 0x01/0x81) | Receive status from wireless devices |
| TL LCD Wireless | 1CBE:0006 | USB Bulk (EP 0x01/0x81) | Wireless LCD panel |

---

## Wired Fan Controller (HID)

### Packet Format (64 bytes, Report ID 0x01)

```
[0]    Report ID: 0x01
[1]    Command
[2]    Reserved: 0x00
[3-4]  Packet number (big-endian u16)
[5]    Payload length (0-58)
[6-63] Payload
```

### Commands

| Cmd  | Name | ACK | Payload |
|------|------|-----|---------|
| 0xA1 | Handshake | Yes | Empty. Response: 3 bytes per fan [flags, rpm_hi, rpm_lo] |
| 0xA3 | SetFanLight | Yes | 20 bytes: LED effect, color, speed, etc. |
| 0xA6 | GetProductInfo | Yes (2 reads) | [isSlave, port]. Returns firmware version strings |
| 0xAA | SetFanSpeed | Yes | [(port << 4) \| fan_index, pwm]. PWM range: 1-100 |
| 0xAD | SetFanGroup | Yes | Lighting group configuration |
| 0xAE | SetFanDirection | Yes | LED direction per fan |
| 0xAF | SetPortDirection | Yes | LED direction per port |
| 0xB0 | SetGroupLight | Yes | Group lighting (20-byte payload) |
| 0xB1 | SetMBRPMSync | Yes | [(sync << 7) \| (port << 4) \| fan_index] |
| 0xB3 | TestLight | Yes | [type, number, isBottom] |
| 0xB4 | BlinkPort | Yes | [port] |

### Handshake Response Fan Entry (3 bytes each)

```
Byte 0:
  bit 7:   detected
  bit 6:   upgrading
  bits 5-4: port (0-3)
  bits 3-0: fan_index (0-15)
Bytes 1-2: RPM (big-endian u16)
```

### Lifecycle

- **No init command needed** — `Init()` is empty in C#
- **No cleanup command** — just close HID handle
- **No keepalive** — but C# polls handshake every 1000ms for RPM + hotplug
- **C# re-sends fan PWM every 1000ms** (temperature curve recalculation)
- **RPM is only readable via handshake** — no dedicated RPM command
- Timeouts: read 100ms, write 1000ms

### Linux HID Note

On Linux, `hidapi` strips Report ID from reads. Our `from_bytes()` handles
both formats (with and without leading 0x01).

---

## Wired LCD (HID)

### Packet Format (512 bytes output / 64 bytes input, Report ID 0x02)

```
[0]     Report ID: 0x02
[1]     Command
[2-5]   Data size (big-endian u32) — total payload across all packets
[6-8]   Packet number (big-endian u24) — incrementing per chunk
[9-10]  Payload length (big-endian u16) — bytes in this packet (max 501)
[11-511] Payload
```

### Commands

| Cmd  | Name | ACK | Description |
|------|------|-----|-------------|
| 0x3C | GetHandshakeInfo | Write-only, then read separately | Returns [mode, frame_hi, frame_lo] at byte offset 11 |
| 0x3D | GetProductInfo | Write-only, then 2 reads | Firmware version |
| 0x3E | ReadSerialNumber | Write-only, then read | [serial(32), port, lcd_index] |
| 0x40 | LCDControl | Yes | 11-byte control setting (see below) |
| 0x41 | WriteJPG | Yes | Static JPEG upload (ACK per 512-byte packet) |
| 0x45 | WriteAVI | Yes | AVI upload to flash (not used by C# for playback) |
| 0x46 | WriteSyncJPG | **No** | Streaming JPEG frame — fire-and-forget |
| 0x47 | WriteBootAVI | Yes | Boot animation (stored in flash) |
| 0x48 | WriteBootJPG | Yes | Boot image (shown on power-up) |

### LCDControl Setting (11 bytes, command 0x40)

```
[0]   Mode (LCDControlMode)
[1-2] JPGIndex (big-endian u16)
[3]   Reserved: 0x00
[4]   Brightness (0-100, default 50)
[5]   VideoFPS (default 30)
[6]   Rotation (0=0, 1=90, 2=180, 3=270)
[7]   EnableTest (0 or 1)
[8-10] TestColor R, G, B
```

All fields are sent atomically — no partial updates.

### LCDControlMode Values

| Value | Name | Purpose |
|-------|------|---------|
| 0 | Reserved | - |
| 1 | ShowJPG | Display the stored JPEG |
| 3 | ShowAVI | Play stored AVI (dead code in C#) |
| 4 | ShowAppSync | App-sync mode (dead code in C#) |
| 5 | LCDSetting | Apply settings (rotation/brightness) without changing display mode |
| 6 | LCDTest | Solid color test fill |

### LCD State Machine

The device boots showing its stored boot JPEG (written via cmd 0x48).

**Static image sequence:**
```
1. LCDControl mode=5 (LCDSetting) — set rotation/brightness
2. WriteJPG cmd 0x41 — upload JPEG (ACK per packet)
3. LCDControl mode=1 (ShowJPG) — tell device to display it
4. WriteBootJPG cmd 0x48 — persist as boot image (optional)
```

**Video streaming sequence:**
```
1. LCDControl mode=5 (LCDSetting) — set rotation/brightness
2. WriteSyncJPG cmd 0x46 loop — fire-and-forget JPEG frames
```

**Critical: the device must be "primed" with at least one WriteJPG (cmd 0x41,
with ACK) before WriteSyncJPG streaming will work.** The C# app never hits
cold-start video because `ExecuteSetting()` always pushes a default JPEG on
first connect. On Linux, we send the first video frame via WriteJPG to prime
the pipeline, then switch to WriteSyncJPG for the rest.

### Handshake Quirk

The device response to cmd 0x3C includes the report ID (0x02) and echoes
the command, but does NOT fill in the payload length field (bytes 9-10 are
zero). The actual data ([mode, frame_hi, frame_lo]) is at byte offset 11
from the raw response. Must read data at fixed offset rather than using
the length field.

---

## Wireless LCD (USB Bulk)

### Encryption

All command headers use DES-CBC encryption:
- Key: `slv3tuzx` (8 bytes)
- IV: `slv3tuzx` (same as key)
- PKCS7 padding: 504-byte plaintext → 512-byte ciphertext

### Command Buffer (504 bytes plaintext)

```
[0]    Command (CmdType)
[1]    Reserved
[2-3]  Magic: 0x1A 0x6D
[4-7]  Timestamp (little-endian u32, seconds since epoch)
[8-11] Data size (big-endian u32, for PushJpg: JPEG length)
[12+]  Data payload (if any)
```

### CmdType Values

| Value | Name | Description |
|-------|------|-------------|
| 10 | GetVer | Get firmware version |
| 11 | Reboot | Reboot the LCD |
| 13 | Rotate | Set rotation (param: 0-3) |
| 14 | Brightness | Set brightness (param: 0-255) |
| 15 | SetFrameRate | Set target FPS |
| 101 | PushJpg | Push JPEG image data |
| 201 | GetPosIndex | Get position index |

### PushJpg Protocol

The C# app always sends exactly **102,400 bytes** per PushJpg:
```
[0-511]       Encrypted command header (504 bytes + PKCS7 padding = 512)
[512-102399]  Raw JPEG data + zero padding to fill 102,400 bytes
```

The device reads the actual JPEG length from bytes [8-11] of the decrypted
header. The fixed transfer size is critical — variable-length transfers
cause the device to only display one frame then stop.

After each PushJpg, the C# app reads and discards any pending response
(`CheckImg` → `Read()`). This provides back-pressure to prevent flooding.

### Video Streaming

```
1. SetFrameRate(fps)
2. Loop: PushJpg(jpeg_frame) — 102,400 bytes each, drain response after
```

Frame pacing: `sleep(1000/fps - elapsed - 1ms)`, clamped to 0 if overdue.

### Recovery

If the device locks up (e.g., from being flooded with too-fast frames),
a USB-level reset is needed — the Reboot command (11) cannot be sent
because USB writes time out. Use `usbreset /dev/bus/usb/XXX/YYY` or
physically replug.

---

## SLV3H Wireless System (3 USB Devices)

### Architecture

Three separate USB devices cooperate:

1. **SLV3H Hub** (1A86:2107, HID) — MAC address lookup
2. **RF TX Dongle** (0416:8040, USB Bulk) — send commands/queries
3. **RF RX Dongle** (0416:8041, USB Bulk) — receive device list/status

Both RF dongles use Bulk transfers (not Interrupt), EP 0x01 out / 0x81 in,
max packet size 64 bytes.

### Hub HID Protocol (Report ID 0x00)

Write 65 bytes: `[0x00, command, ...]` (byte 0 is Report ID 0).

| Command | Description |
|---------|-------------|
| 0x1C | Get MAC address. Response: MAC at bytes [2..8] |

### TX Dongle — Query Master

Write 64 bytes, read 64 bytes:

```
Query: [0x11, channel, 0, 0, ...]
Response:
  [2-7]   Master MAC (6 bytes)
  [8-11]  System clock (LE u32)
  [12-13] Firmware version (LE u16)
```

Default channel: `0x31`.

### RX Dongle — Device List

Write 64 bytes: `[0x10, page_count, ...]`
Read: 434 bytes per page (64-byte USB chunks).

Each device entry is 42 bytes:

```
[0-5]   Device MAC
[6-11]  Master MAC
[12]    Channel
[13]    RX type
[14-17] System time (LE u32)
[18]    Device type (0=TLFan, 2=SLStrimer, etc.)
[19]    Fan count
[20-23] Effect indices (4 bytes)
[24-27] Fan types (4 bytes)
[28-35] Fan speeds (4x u16 LE)
[36-39] Fan PWM (4 bytes)
[40]    Command sequence
[41]    Marker: 0x1C
```

Bound devices have a master MAC matching the hub's master.

### RF Packet Format (240 bytes, fragmented into 4x 60-byte USB chunks)

USB chunk format:
```
[0]    0x10 (fixed)
[1]    Fragment index (0-3)
[2]    Channel
[3]    RX type
[4-63] 60 bytes of RF payload
```

### RF Bind/PWM Command

RF payload byte layout:
```
[0]    0x12 (RF command)
[1]    0x10 (bind sub-command)
[2-7]  Target device MAC
[8-13] Master MAC
[14]   RX type
[15]   Channel
[16]   Slave index (>0 = bound)
[17-20] Fan PWM values (4 bytes)
```

### Initialization Sequence

```
1. Hub: Get MAC (cmd 0x1C)
2. TX: Query master (cmd 0x11, channel 0x31) → get master MAC, clock, firmware
3. RX: Get device list (cmd 0x10) → enumerate all wireless devices
4. TX: Send RF bind/PWM packets to control individual devices
```

---

## ffmpeg Integration

Both wired and wireless LCD use ffmpeg for media conversion:

**Image (any format → 400x400 JPEG):**
```
ffmpeg -i INPUT -vf "scale=400:400:force_original_aspect_ratio=increase,crop=400:400"
       -frames:v 1 -f mjpeg -q:v 5 pipe:1
```

**Video (any format → MJPEG stream):**
```
ffmpeg -i INPUT -vf "scale=400:400:force_original_aspect_ratio=increase,crop=400:400"
       -r FPS -f mjpeg -q:v 5 pipe:1
```

JPEG frames are parsed from the MJPEG stream by finding FF D8 (start) and
FF D9 (end) markers. Max frame size: 500,000 bytes.

Center-crop (not letterbox) is used to fill the 400x400 display.

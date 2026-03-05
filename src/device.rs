use std::fmt;

use hidapi::HidApi;

use crate::Result;

pub mod known {
    pub const TL_FAN_VID: u16 = 0x0416;
    pub const TL_FAN_PID: u16 = 0x7372;
    pub const TL_FAN_USAGE_PAGE: u16 = 0xFF0B;

    pub const TL_LCD_WIRED_VID: u16 = 0x04FC;
    pub const TL_LCD_WIRED_PID: u16 = 0x7393;

    // The wireless LCD has two USB devices:
    // 1CBE:0006 — raw USB LCD (bulk transfers, for video/images)
    // 1A86:2107 — SLV3H HID interface (for fan/LED control)
    pub const TL_LCD_WIRELESS_USB_VID: u16 = 0x1CBE;
    pub const TL_LCD_WIRELESS_USB_PID: u16 = 0x0006;

    pub const SLV3H_VID: u16 = 0x1A86;
    pub const SLV3H_PID: u16 = 0x2107;

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
    Slv3h,
    RfTxDongle,
    RfRxDongle,
}

impl fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceKind::TlFanController => write!(f, "TL Fan Controller"),
            DeviceKind::TlLcdWired => write!(f, "TL LCD (Wired)"),
            DeviceKind::TlLcdWireless => write!(f, "TL LCD (Wireless)"),
            DeviceKind::Slv3h => write!(f, "SLV3H Wireless Hub"),
            DeviceKind::RfTxDongle => write!(f, "RF TX Dongle"),
            DeviceKind::RfRxDongle => write!(f, "RF RX Dongle"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub kind: DeviceKind,
    pub path: String,
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
}

impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{:04x}:{:04x}] {}", self.kind, self.vid, self.pid, self.path)
    }
}

pub fn discover() -> Result<Vec<DeviceInfo>> {
    let api = HidApi::new()?;
    let mut devices = Vec::new();

    for dev in api.device_list() {
        let vid = dev.vendor_id();
        let pid = dev.product_id();

        let kind = match (vid, pid) {
            (known::TL_FAN_VID, known::TL_FAN_PID) => {
                if dev.usage_page() != known::TL_FAN_USAGE_PAGE {
                    continue;
                }
                DeviceKind::TlFanController
            }
            (known::TL_LCD_WIRED_VID, known::TL_LCD_WIRED_PID) => DeviceKind::TlLcdWired,
            (known::SLV3H_VID, known::SLV3H_PID) => DeviceKind::Slv3h,
            (known::RF_TX_VID, known::RF_TX_PID) => DeviceKind::RfTxDongle,
            (known::RF_RX_VID, known::RF_RX_PID) => DeviceKind::RfRxDongle,
            _ => continue,
        };

        devices.push(DeviceInfo {
            kind,
            path: dev.path().to_string_lossy().into_owned(),
            vid,
            pid,
            serial: dev.serial_number().map(String::from),
        });
    }

    Ok(devices)
}

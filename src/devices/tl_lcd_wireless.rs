use hidapi::HidApi;

use crate::device::DeviceInfo;
use crate::protocol::{lcd, packet::LcdPacket};
use crate::transport::hid::HidTransport;
use crate::Result;

/// Wireless TL LCD (1CBE:0006).
/// HID reports work for control commands (handshake, lcd control).
/// Video/image streaming uses USB bulk with DES encryption — not yet implemented.
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
        let resp = self.transport.lcd_write_read(&packets[0].to_bytes())?;
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

    // TODO: send_jpg, send_avi via USB bulk + DES encryption
}

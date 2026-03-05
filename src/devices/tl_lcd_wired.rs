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

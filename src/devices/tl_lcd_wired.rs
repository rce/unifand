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
        // C# sends handshake as write-only, then reads separately with longer timeout.
        // The device needs time to prepare the response.
        self.transport.lcd_write(&packets[0].to_bytes())?;
        let resp = self.transport.raw_read(64, 1000)?;
        let mut buf = [0u8; 64];
        let len = resp.len().min(64);
        buf[..len].copy_from_slice(&resp[..len]);
        let resp = LcdPacket::from_bytes(&buf)?;
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

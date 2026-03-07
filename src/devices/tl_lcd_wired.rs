use hidapi::HidApi;

use crate::Result;
use crate::device::DeviceInfo;
use crate::protocol::lcd;
use crate::transport::hid::HidTransport;

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
        self.transport.lcd_write(&packets[0].to_bytes())?;
        let resp = self.transport.raw_read(64, 1000)?;
        // Device response: [report_id, cmd, ...header..., data at byte 11+]
        // The device doesn't fill in the payload length field, so read data
        // directly from offset 11 (after report ID + 10-byte header).
        let data_offset = if resp[0] == 0x02 { 11 } else { 10 };
        if resp.len() <= data_offset {
            return Err(crate::Error::InvalidResponse(
                "handshake response too short".into(),
            ));
        }
        lcd::parse_handshake(&resp[data_offset..])
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

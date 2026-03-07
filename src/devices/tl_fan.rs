use hidapi::HidApi;

use crate::Result;
use crate::device::{DeviceInfo, known};
use crate::protocol::{fan, packet::LedPacket};
use crate::transport::hid::HidTransport;

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

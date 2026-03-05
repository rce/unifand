use hidapi::{HidApi, HidDevice};

use crate::error::Result;
use crate::protocol::packet::{LCD_INPUT_LEN, LCD_OUTPUT_LEN, LED_PACKET_LEN};

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

    /// Send a 64-byte LED/fan packet and read 64-byte response. Read timeout: 100ms.
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

    /// Send a 512-byte LCD packet and read 64-byte response. Read timeout: 200ms.
    pub fn lcd_write_read(&self, data: &[u8; LCD_OUTPUT_LEN]) -> Result<[u8; LCD_INPUT_LEN]> {
        self.device.write(data)?;
        let mut buf = [0u8; LCD_INPUT_LEN];
        let n = self.device.read_timeout(&mut buf, 200)?;
        if n == 0 {
            return Err(crate::Error::Timeout);
        }
        Ok(buf)
    }

    /// Send a 512-byte LCD packet without reading response (for sync JPG fire-and-forget).
    pub fn lcd_write(&self, data: &[u8; LCD_OUTPUT_LEN]) -> Result<()> {
        self.device.write(data)?;
        Ok(())
    }

    /// Raw write for devices using Report ID 0 (e.g. SLV3H).
    pub fn raw_write(&self, data: &[u8]) -> Result<()> {
        self.device.write(data)?;
        Ok(())
    }

    /// Raw read with configurable timeout and buffer size.
    pub fn raw_read(&self, len: usize, timeout_ms: i32) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = self.device.read_timeout(&mut buf, timeout_ms)?;
        if n == 0 {
            return Err(crate::Error::Timeout);
        }
        buf.truncate(n);
        Ok(buf)
    }
}

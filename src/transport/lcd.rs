/// Transport for wireless LCD display (1CBE:0006) via raw USB bulk.
///
/// The LCD accepts DES-CBC encrypted 504-byte command headers,
/// optionally followed by raw payload data (e.g. JPG image bytes).

use std::time::Duration;

use rusb::{Context, DeviceHandle, UsbContext};

use crate::device::known;
use crate::transport::usb;
use crate::Result;

const ENDPOINT_OUT: u8 = 0x01;
const ENDPOINT_IN: u8 = 0x81;
const INTERFACE: u8 = 0;
const TIMEOUT: Duration = Duration::from_millis(500);
/// LCD command types (from decompiled lcd207 CmdType enum).
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum LcdCmd {
    GetVer = 10,
    Reboot = 11,
    Rotate = 13,
    Brightness = 14,
    SetFrameRate = 15,
    PushJpg = 101,
    GetPosIndex = 201,
}

pub struct LcdTransport {
    handle: DeviceHandle<Context>,
}

impl LcdTransport {
    pub fn open() -> Result<Self> {
        let ctx = Context::new()?;
        let device = ctx
            .devices()?
            .iter()
            .find(|d| {
                d.device_descriptor()
                    .map(|desc| {
                        desc.vendor_id() == known::TL_LCD_WIRELESS_USB_VID
                            && desc.product_id() == known::TL_LCD_WIRELESS_USB_PID
                    })
                    .unwrap_or(false)
            })
            .ok_or_else(|| crate::Error::DeviceNotFound("wireless LCD 1CBE:0006".into()))?;

        let handle = device.open()?;
        if handle.kernel_driver_active(INTERFACE).unwrap_or(false) {
            handle.detach_kernel_driver(INTERFACE)?;
        }
        handle.claim_interface(INTERFACE)?;

        Ok(Self { handle })
    }

    /// Send a simple command with one byte parameter.
    pub fn send_cmd(&self, cmd: LcdCmd, param: u8) -> Result<()> {
        let mut buf = usb::build_command_buffer(cmd as u8, None);
        buf[8] = param;
        let encrypted = usb::encrypt_pkcs7(&buf);
        self.write_bulk(&encrypted)?;
        Ok(())
    }

    /// Send a command without parameters.
    pub fn send_cmd_bare(&self, cmd: LcdCmd) -> Result<()> {
        let buf = usb::build_command_buffer(cmd as u8, None);
        let encrypted = usb::encrypt_pkcs7(&buf);
        self.write_bulk(&encrypted)?;
        Ok(())
    }

    /// Push a JPG image to the LCD.
    /// Protocol: encrypted header (512 bytes) + raw JPG data.
    pub fn push_jpg(&self, jpg_data: &[u8]) -> Result<()> {
        // Build header with data length only (image goes after the encrypted header)
        let mut buf = usb::build_command_buffer(LcdCmd::PushJpg as u8, None);
        let len = jpg_data.len() as u32;
        buf[8] = (len >> 24) as u8;
        buf[9] = ((len >> 16) & 0xFF) as u8;
        buf[10] = ((len >> 8) & 0xFF) as u8;
        buf[11] = (len & 0xFF) as u8;
        let encrypted = usb::encrypt_pkcs7(&buf);

        // Full transfer: encrypted header + raw image data
        let mut transfer = Vec::with_capacity(encrypted.len() + jpg_data.len());
        transfer.extend_from_slice(&encrypted);
        transfer.extend_from_slice(jpg_data);

        self.write_bulk(&transfer)?;
        Ok(())
    }

    /// Read a response from the LCD (512 bytes max).
    pub fn read_response(&self) -> Result<Vec<u8>> {
        let mut buf = [0u8; 512];
        let n = self.handle.read_bulk(ENDPOINT_IN, &mut buf, TIMEOUT)?;
        let encrypted = &buf[..n];
        // Decrypt the response
        let decrypted = usb::decrypt(encrypted);
        Ok(decrypted)
    }

    fn write_bulk(&self, data: &[u8]) -> Result<()> {
        self.handle.write_bulk(ENDPOINT_OUT, data, TIMEOUT)?;
        Ok(())
    }
}

impl Drop for LcdTransport {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(INTERFACE);
    }
}

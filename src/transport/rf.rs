/// Transport for SLV3H RF TX/RX dongles via raw USB (rusb).
///
/// TX dongle (0416:8040): sends RF commands, queries master MAC
/// RX dongle (0416:8041): receives device list/status

use std::time::Duration;

use rusb::{Context, DeviceHandle, UsbContext};

use crate::device::known;
use crate::Result;

const ENDPOINT_OUT: u8 = 0x01;
const ENDPOINT_IN: u8 = 0x81;
const INTERFACE: u8 = 0;
const TIMEOUT: Duration = Duration::from_millis(200);

pub struct RfTransport {
    handle: DeviceHandle<Context>,
    claimed: bool,
}

impl RfTransport {
    fn open_device(vid: u16, pid: u16) -> Result<Self> {
        let ctx = Context::new()?;
        let device = ctx
            .devices()?
            .iter()
            .find(|d| {
                d.device_descriptor()
                    .map(|desc| desc.vendor_id() == vid && desc.product_id() == pid)
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                crate::Error::DeviceNotFound(format!("USB device {:04x}:{:04x}", vid, pid))
            })?;

        let handle = device.open()?;

        let claimed = if handle.kernel_driver_active(INTERFACE).unwrap_or(false) {
            handle.detach_kernel_driver(INTERFACE)?;
            handle.claim_interface(INTERFACE)?;
            true
        } else {
            handle.claim_interface(INTERFACE)?;
            true
        };

        Ok(Self { handle, claimed })
    }

    /// Open the RF TX dongle (0416:8040).
    pub fn open_tx() -> Result<Self> {
        Self::open_device(known::RF_TX_VID, known::RF_TX_PID)
    }

    /// Open the RF RX dongle (0416:8041).
    pub fn open_rx() -> Result<Self> {
        Self::open_device(known::RF_RX_VID, known::RF_RX_PID)
    }

    /// Write a 64-byte packet to the dongle.
    pub fn write(&self, data: &[u8; 64]) -> Result<()> {
        self.handle
            .write_bulk(ENDPOINT_OUT, data, TIMEOUT)?;
        Ok(())
    }

    /// Write and then read a response.
    pub fn write_read(&self, data: &[u8; 64]) -> Result<Vec<u8>> {
        self.write(data)?;
        self.read(64, TIMEOUT)
    }

    /// Read up to `len` bytes from the dongle.
    pub fn read(&self, len: usize, timeout: Duration) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = self
            .handle
            .read_bulk(ENDPOINT_IN, &mut buf, timeout)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Read a large response in 64-byte chunks (matching endpoint max packet size).
    pub fn read_large(&self, len: usize, timeout: Duration) -> Result<Vec<u8>> {
        let mut result = Vec::with_capacity(len);

        while result.len() < len {
            let mut buf = [0u8; 64];
            match self
                .handle
                .read_bulk(ENDPOINT_IN, &mut buf, timeout)
            {
                Ok(n) => {
                    result.extend_from_slice(&buf[..n]);
                    if n < 64 {
                        // Short read = last packet
                        break;
                    }
                }
                Err(rusb::Error::Timeout) => break,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(result)
    }
}

impl Drop for RfTransport {
    fn drop(&mut self) {
        if self.claimed {
            let _ = self.handle.release_interface(INTERFACE);
        }
    }
}

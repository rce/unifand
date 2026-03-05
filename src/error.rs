use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HID error: {0}")]
    Hid(#[from] hidapi::HidError),

    #[error("USB error: {0}")]
    Usb(#[from] rusb::Error),

    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("timeout waiting for device response")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, Error>;

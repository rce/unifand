use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum Request {
    Status,
    DisplayImage {
        path: String,
    },
    DisplayVideo {
        path: String,
        fps: u8,
        loop_video: bool,
    },
    DisplayBrightness {
        level: u8,
    },
    DisplayRotate {
        rotation: u16,
    },
    DisplayReset,
    SetFanSpeed {
        mac: String,
        pwm: u8,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<DaemonStatus>,
}

impl Response {
    pub fn ok() -> Self {
        Self {
            ok: true,
            error: None,
            status: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(msg.into()),
            status: None,
        }
    }

    pub fn with_status(status: DaemonStatus) -> Self {
        Self {
            ok: true,
            error: None,
            status: Some(status),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub uptime_secs: u64,
    pub wireless_lcds: Vec<WirelessLcdState>,
    pub wired_lcds: Vec<WiredLcdState>,
    pub display: DisplayState,
    pub wireless_fans: Vec<WirelessFanState>,
    #[serde(default)]
    pub config_fans: Vec<FanConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WirelessLcdState {
    pub serial: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WiredLcdState {
    pub port: u8,
    pub lcd_index: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayState {
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<u8>,
    pub looping: bool,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            mode: "idle".into(),
            source: None,
            fps: None,
            looping: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WirelessFanState {
    pub mac: String,
    pub fan_count: u8,
    pub fans: Vec<FanReading>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FanReading {
    pub rpm: u16,
    pub pwm: u8,
}

/// Socket path for the daemon.
pub fn socket_path() -> std::path::PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR must be set");
    std::path::PathBuf::from(runtime_dir).join("unifand.sock")
}

/// Config file path (~/.config/unifand/config.json).
pub fn config_path() -> std::path::PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").expect("HOME must be set");
        format!("{home}/.config")
    });
    std::path::PathBuf::from(config_dir).join("unifand/config.json")
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub fans: Vec<FanConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanConfig {
    /// Wireless LCD serial (for wireless matching).
    #[serde(default)]
    pub serial: String,
    /// USB port number (for wired LCD matching).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u8>,
    /// LCD index within the port (for wired LCD matching).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lcd_index: Option<u8>,
    /// Video playback config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoConfig {
    pub path: String,
    #[serde(default = "default_fps")]
    pub fps: u8,
}

fn default_fps() -> u8 {
    20
}

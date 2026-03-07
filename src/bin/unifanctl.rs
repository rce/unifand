use std::io::{BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

use clap::{Parser, Subcommand};
use unifand::device::DeviceKind;
use unifand::devices::slv3h::Slv3hController;
use unifand::devices::tl_fan::TlFanController;
use unifand::devices::tl_lcd_wired::TlLcdWired;
use unifand::ipc::{self, DaemonStatus, Request, Response};
use unifand::protocol::lcd::{LcdControlSetting, LcdMode, ScreenRotation};
use unifand::protocol::slv3h;
use unifand::transport::lcd::{LcdCmd, LcdTransport};

#[derive(Parser)]
#[command(name = "unifanctl", about = "Control Lian Li Uni Fans")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show daemon status (connect to running unifand)
    Status,
    /// List all detected Lian Li devices
    Discover,
    /// Fan control commands (wired TL Fan Controller)
    Fan {
        #[command(subcommand)]
        command: FanCommands,
    },
    /// Wireless fan control (SLV3H hub + RF dongles)
    Wireless {
        #[command(subcommand)]
        command: WirelessCommands,
    },
    /// LCD display commands (auto-detects wired or wireless LCD)
    Display {
        #[command(subcommand)]
        command: DisplayCommands,
    },
}

#[derive(Subcommand)]
enum FanCommands {
    /// Show detected fans and RPM
    Status,
    /// Set fan speed
    SetSpeed { port: u8, fan: u8, pwm: u8 },
    /// Blink a port's LEDs for identification
    Blink { port: u8 },
}

#[derive(Subcommand)]
enum WirelessCommands {
    /// Initialize and show hub info
    Init,
    /// Show all wireless devices (fans, strimers, etc.)
    Status,
    /// Set fan PWM for all fans on a device (by MAC address)
    SetSpeed {
        /// Device MAC address (e.g. "aa:bb:cc:dd:ee:ff")
        mac: String,
        /// PWM value 0-100
        pwm: u8,
    },
}

#[derive(Subcommand)]
enum DisplayCommands {
    /// Reboot the LCD (wireless only, useful to recover from stuck state)
    Reset,
    /// Set LCD brightness
    Brightness { level: u8 },
    /// Rotate LCD display
    Rotate {
        /// Rotation: 0-3 for wireless (0°,90°,180°,270°) or 0/90/180/270 for wired
        rotation: u16,
    },
    /// Push an image to the LCD (any format, converted via ffmpeg to 400x400 JPG)
    Image { path: String },
    /// Stream a video to the LCD (any format, converted via ffmpeg to 400x400 JPG frames)
    Video {
        path: String,
        /// Target FPS (default: 20)
        #[arg(long, default_value_t = 20)]
        fps: u8,
        /// Loop the video
        #[arg(long)]
        r#loop: bool,
    },
}

/// Which LCD type we found.
enum Lcd {
    Wireless(Vec<LcdTransport>),
    Wired(TlLcdWired),
}

/// Convert any image to 400x400 JPG using ffmpeg.
fn convert_image_to_jpg(path: &str) -> anyhow::Result<Vec<u8>> {
    let output = Command::new("ffmpeg")
        .args([
            "-i",
            path,
            "-vf",
            "scale=400:400:force_original_aspect_ratio=increase,crop=400:400",
            "-frames:v",
            "1",
            "-f",
            "mjpeg",
            "-q:v",
            "5",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;

    if !output.status.success() {
        anyhow::bail!("ffmpeg failed to convert image");
    }
    Ok(output.stdout)
}

/// Try to open any available LCD (wireless first, then wired).
fn open_lcd() -> anyhow::Result<Lcd> {
    // Try wireless LCDs first (USB bulk) — each fan LCD is a separate USB device
    if let Ok(lcds) = LcdTransport::open_all() {
        if !lcds.is_empty() {
            println!("Found {} wireless LCD(s)", lcds.len());
            return Ok(Lcd::Wireless(lcds));
        }
    }

    // Try wired LCD (HID)
    let devices = unifand::discover()?;
    if let Some(lcd_info) = devices
        .iter()
        .find(|d| matches!(d.kind, DeviceKind::TlLcdWired))
    {
        let api = hidapi::HidApi::new()?;
        let lcd = TlLcdWired::open(&api, lcd_info)?;
        match lcd.handshake() {
            Ok(info) => eprintln!(
                "Handshake OK: mode={}, frame_index={}",
                info.mode, info.frame_index
            ),
            Err(e) => eprintln!("Handshake failed: {e}"),
        }
        println!("Found wired LCD");
        return Ok(Lcd::Wired(lcd));
    }

    anyhow::bail!("No LCD display found (checked wireless USB and wired HID)")
}

/// Send a request to the daemon and return the response.
fn daemon_request(req: &Request) -> anyhow::Result<Response> {
    let sock_path = ipc::socket_path();
    let mut stream = UnixStream::connect(&sock_path).map_err(|e| {
        anyhow::anyhow!("cannot connect to unifand at {}: {e}", sock_path.display())
    })?;

    let json = serde_json::to_string(req)?;
    writeln!(stream, "{json}")?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut reader = std::io::BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;

    let resp: Response = serde_json::from_str(&line)?;
    Ok(resp)
}

/// Check if the daemon is running.
fn daemon_available() -> bool {
    ipc::socket_path().exists()
}

fn print_status(status: &DaemonStatus) {
    println!("unifand.service - Lian Li Uni Fan Daemon");

    let secs = status.uptime_secs;
    let uptime = if secs >= 3600 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    };
    println!("  Active: running (uptime: {uptime})");

    let lcd_desc = if status.wireless_lcds > 0 {
        format!("{} wireless", status.wireless_lcds)
    } else if status.wired_lcd {
        "1 wired".into()
    } else {
        "none".into()
    };
    println!("  LCDs: {lcd_desc}");
    if !status.lcd_serials.is_empty() {
        for (i, serial) in status.lcd_serials.iter().enumerate() {
            let s = serial.as_deref().unwrap_or("(none)");
            println!("    LCD {i}: serial={s}");
        }
    }

    let display = &status.display;
    let display_desc = match display.mode.as_str() {
        "idle" => "idle".into(),
        "image" => {
            format!("image {}", display.source.as_deref().unwrap_or("?"))
        }
        "video" => {
            let src = display.source.as_deref().unwrap_or("?");
            let fps = display
                .fps
                .map(|f| format!(" @ {f}fps"))
                .unwrap_or_default();
            let looping = if display.looping { " (looping)" } else { "" };
            format!("video {src}{fps}{looping}")
        }
        other => other.into(),
    };
    println!("  Display: {display_desc}");

    if !status.wireless_fans.is_empty() {
        println!();
        println!("  Wireless Devices:");
        for device in &status.wireless_fans {
            println!("    Receiver {}", device.mac);
            for (i, f) in device.fans.iter().enumerate() {
                println!("      Fan {i}: RPM={}, PWM={}", f.rpm, f.pwm);
            }
        }
    }

    // Show config entries matched to LCD serials
    if !status.config_fans.is_empty() {
        println!();
        println!("  Fan Config:");
        for c in &status.config_fans {
            let matched = status
                .lcd_serials
                .iter()
                .any(|s| s.as_deref() == Some(&c.serial));
            let video_desc = match &c.video {
                Some(v) => format!("video {}@{}fps", v, c.fps),
                None => "no video".into(),
            };
            if matched {
                println!("    LCD {}: {}", c.serial, video_desc);
            } else {
                println!("    Config {:?}: fan not found (disconnected?)", c.serial);
            }
        }
    }

    // Warn about connected LCDs with no config entry
    let unconfigured: Vec<_> = status
        .lcd_serials
        .iter()
        .filter_map(|s| s.as_deref())
        .filter(|s| !status.config_fans.iter().any(|c| c.serial == *s))
        .collect();
    if !unconfigured.is_empty() {
        let config_path = ipc::config_path();
        println!();
        println!("  Unconfigured LCDs:");
        for s in unconfigured {
            println!("    LCD {s}: no config (add to {})", config_path.display());
        }
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status => {
            let resp = daemon_request(&Request::Status)?;
            if let Some(status) = resp.status {
                print_status(&status);
            } else if let Some(err) = resp.error {
                anyhow::bail!("{err}");
            }
        }
        Commands::Discover => {
            let devices = unifand::discover()?;
            if devices.is_empty() {
                println!("No Lian Li devices found.");
            } else {
                for dev in &devices {
                    println!("{dev}");
                }
            }
        }
        Commands::Fan { command } => {
            let api = hidapi::HidApi::new()?;
            let devices = unifand::discover()?;
            let fan_info = devices
                .iter()
                .find(|d| matches!(d.kind, DeviceKind::TlFanController))
                .ok_or_else(|| anyhow::anyhow!("No wired fan controller found"))?;
            let controller = TlFanController::open(&api, fan_info)?;

            match command {
                FanCommands::Status => {
                    let fans = controller.handshake()?;
                    if fans.is_empty() {
                        println!("No fans detected.");
                    } else {
                        for fan in &fans {
                            println!(
                                "Port {} Fan {} — detected: {}, RPM: {}",
                                fan.port, fan.fan_index, fan.detected, fan.rpm
                            );
                        }
                    }
                }
                FanCommands::SetSpeed { port, fan, pwm } => {
                    controller.set_fan_speed(port, fan, pwm)?;
                    println!("Set port {port} fan {fan} to PWM {pwm}");
                }
                FanCommands::Blink { port } => {
                    controller.blink_port(port)?;
                    println!("Blinking port {port}");
                }
            }
        }
        Commands::Wireless { command } => {
            let api = hidapi::HidApi::new()?;
            let devices = unifand::discover()?;
            let hub_info = devices
                .iter()
                .find(|d| matches!(d.kind, DeviceKind::Slv3h))
                .ok_or_else(|| anyhow::anyhow!("No SLV3H wireless hub found"))?;

            let mut controller = Slv3hController::open(&api, hub_info)?;

            match command {
                WirelessCommands::Init => {
                    let master = controller.init()?;
                    println!("Master MAC: {}", slv3h::format_mac(&master.mac));
                    println!("System clock: {}", master.sys_clock);
                    println!("Firmware: {:#06x}", master.firmware_version);
                }
                WirelessCommands::Status => {
                    controller.init()?;
                    let rf_devices = controller.get_device_list()?;
                    if rf_devices.is_empty() {
                        println!("No wireless devices found.");
                    } else {
                        let master = controller.master_info().unwrap();
                        println!("Master: {}", slv3h::format_mac(&master.mac));
                        println!();
                        for dev in &rf_devices {
                            println!(
                                "Device {} — type: {:?}, fans: {}, bound: {}",
                                slv3h::format_mac(&dev.mac),
                                dev.dev_type,
                                dev.fan_num,
                                dev.bound,
                            );
                            for i in 0..dev.fan_num as usize {
                                if i < 4 {
                                    println!(
                                        "  Fan {}: RPM={}, PWM={}",
                                        i, dev.fan_speeds[i], dev.fan_pwm[i]
                                    );
                                }
                            }
                        }
                    }
                }
                WirelessCommands::SetSpeed { mac, pwm } => {
                    controller.init()?;
                    let rf_devices = controller.get_device_list()?;

                    let target = rf_devices
                        .iter()
                        .find(|d| slv3h::format_mac(&d.mac) == mac)
                        .ok_or_else(|| anyhow::anyhow!("Device {mac} not found"))?;

                    let pwm_all = [pwm; 4];
                    controller.set_fan_pwm(target, &pwm_all)?;
                    println!("Set all fans on {mac} to PWM {pwm}");
                }
            }
        }
        Commands::Display { command } => {
            // Route through daemon if running
            if daemon_available() {
                let req = match &command {
                    DisplayCommands::Reset => Request::DisplayReset,
                    DisplayCommands::Brightness { level } => {
                        Request::DisplayBrightness { level: *level }
                    }
                    DisplayCommands::Rotate { rotation } => Request::DisplayRotate {
                        rotation: *rotation,
                    },
                    DisplayCommands::Image { path } => Request::DisplayImage { path: path.clone() },
                    DisplayCommands::Video { path, fps, r#loop } => Request::DisplayVideo {
                        path: path.clone(),
                        fps: *fps,
                        loop_video: *r#loop,
                    },
                };
                let resp = daemon_request(&req)?;
                if resp.ok {
                    println!("OK");
                } else if let Some(err) = resp.error {
                    anyhow::bail!("{err}");
                }
                return Ok(());
            }

            // Direct hardware access (no daemon)
            let lcd = open_lcd()?;

            match command {
                DisplayCommands::Reset => match &lcd {
                    Lcd::Wireless(screens) => {
                        for w in screens {
                            w.send_cmd_bare(LcdCmd::Reboot)?;
                        }
                        println!("Sent reboot command to {} display(s)", screens.len());
                    }
                    Lcd::Wired(_) => {
                        anyhow::bail!("Reset not supported on wired LCD");
                    }
                },
                DisplayCommands::Brightness { level } => {
                    match &lcd {
                        Lcd::Wireless(screens) => {
                            for w in screens {
                                w.send_cmd(LcdCmd::Brightness, level)?;
                            }
                        }
                        Lcd::Wired(w) => {
                            w.set_control(&LcdControlSetting {
                                mode: LcdMode::LcdSetting,
                                jpg_index: 0,
                                brightness: level,
                                video_fps: 0,
                                rotation: ScreenRotation::Deg0,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                        }
                    }
                    println!("Set display brightness to {level}");
                }
                DisplayCommands::Rotate { rotation } => match &lcd {
                    Lcd::Wireless(screens) => {
                        if rotation > 3 {
                            anyhow::bail!("Rotation must be 0-3 (0°, 90°, 180°, 270°)");
                        }
                        for w in screens {
                            w.send_cmd(LcdCmd::Rotate, rotation as u8)?;
                        }
                        println!("Rotated {} display(s) to {}°", screens.len(), rotation * 90);
                    }
                    Lcd::Wired(w) => {
                        let rot = match rotation {
                            0 => ScreenRotation::Deg0,
                            90 => ScreenRotation::Deg90,
                            180 => ScreenRotation::Deg180,
                            270 => ScreenRotation::Deg270,
                            _ => anyhow::bail!(
                                "Invalid rotation: {rotation}. Use 0, 90, 180, or 270."
                            ),
                        };
                        w.set_control(&LcdControlSetting {
                            mode: LcdMode::LcdSetting,
                            jpg_index: 0,
                            brightness: 100,
                            video_fps: 0,
                            rotation: rot,
                            enable_test: false,
                            test_color: (0, 0, 0),
                        })?;
                        println!("Rotated display to {rotation}°");
                    }
                },
                DisplayCommands::Image { path } => {
                    let jpg_data = convert_image_to_jpg(&path)?;
                    match &lcd {
                        Lcd::Wireless(screens) => {
                            for w in screens {
                                w.push_jpg(&jpg_data)?;
                            }
                        }
                        Lcd::Wired(w) => {
                            w.send_jpg(&jpg_data)?;
                            w.set_control(&LcdControlSetting {
                                mode: LcdMode::ShowJpg,
                                jpg_index: 0,
                                brightness: 100,
                                video_fps: 0,
                                rotation: ScreenRotation::Deg0,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                        }
                    }
                    println!("Pushed image to display ({} bytes)", jpg_data.len());
                }
                DisplayCommands::Video { path, fps, r#loop } => {
                    if fps > 60 {
                        anyhow::bail!(
                            "FPS too high (max 60) — the LCD can't keep up and will lock up"
                        );
                    }
                    if fps > 30 {
                        eprintln!("Warning: FPS above 30 may cause the LCD to lock up");
                    }

                    match &lcd {
                        Lcd::Wireless(screens) => {
                            for w in screens {
                                w.send_cmd(LcdCmd::SetFrameRate, fps)?;
                            }
                        }
                        Lcd::Wired(w) => {
                            w.set_control(&LcdControlSetting {
                                mode: LcdMode::ShowJpg,
                                jpg_index: 0,
                                brightness: 100,
                                video_fps: fps,
                                rotation: ScreenRotation::Deg0,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                        }
                    }

                    let frame_duration = std::time::Duration::from_millis(1000 / fps as u64);

                    loop {
                        let mut child = Command::new("ffmpeg")
                            .args([
                                "-i",
                                &path,
                                "-vf",
                                "scale=400:400:force_original_aspect_ratio=increase,crop=400:400",
                                "-r",
                                &fps.to_string(),
                                "-f",
                                "mjpeg",
                                "-q:v",
                                "5",
                                "pipe:1",
                            ])
                            .stdin(Stdio::null())
                            .stdout(Stdio::piped())
                            .stderr(Stdio::null())
                            .spawn()?;

                        let stdout = child.stdout.take().unwrap();
                        let mut reader = std::io::BufReader::new(stdout);
                        let mut frame_count: u64 = 0;

                        // MJPEG stream: each frame starts with FF D8 and ends with FF D9
                        loop {
                            let frame_start = std::time::Instant::now();

                            match read_jpeg_frame(&mut reader) {
                                Ok(frame) => {
                                    let result = match &lcd {
                                        Lcd::Wireless(screens) => {
                                            // Push same frame to all screens
                                            let mut r = Ok(());
                                            for w in screens {
                                                if let Err(e) = w.push_jpg(&frame) {
                                                    r = Err(e);
                                                }
                                            }
                                            r
                                        }
                                        Lcd::Wired(w) => {
                                            if frame_count == 0 {
                                                w.send_jpg(&frame)
                                            } else {
                                                w.send_sync_jpg(&frame)
                                            }
                                        }
                                    };
                                    if let Err(e) = result {
                                        eprintln!("Error pushing frame: {e}");
                                        break;
                                    }
                                    frame_count += 1;
                                    if frame_count % 100 == 0 {
                                        eprintln!("Streamed {frame_count} frames");
                                    }

                                    let elapsed = frame_start.elapsed();
                                    if elapsed < frame_duration {
                                        std::thread::sleep(frame_duration - elapsed);
                                    }
                                }
                                Err(_) => break,
                            }
                        }

                        let _ = child.wait();
                        println!("Streamed {frame_count} frames");

                        if !r#loop {
                            break;
                        }
                        println!("Looping...");
                    }
                }
            }
        }
    }

    Ok(())
}

/// Read a single JPEG frame from an MJPEG stream.
/// JPEG frames start with FF D8 and end with FF D9.
fn read_jpeg_frame<R: Read>(reader: &mut R) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(32768);
    let mut byte = [0u8; 1];

    // Find start marker FF D8
    let mut found_ff = false;
    loop {
        if reader.read_exact(&mut byte).is_err() {
            anyhow::bail!("end of stream");
        }
        if found_ff && byte[0] == 0xD8 {
            buf.push(0xFF);
            buf.push(0xD8);
            break;
        }
        found_ff = byte[0] == 0xFF;
    }

    // Read until end marker FF D9
    loop {
        if reader.read_exact(&mut byte).is_err() {
            anyhow::bail!("end of stream");
        }
        buf.push(byte[0]);
        if buf.len() >= 2 && buf[buf.len() - 2] == 0xFF && buf[buf.len() - 1] == 0xD9 {
            break;
        }
        if buf.len() > 500_000 {
            anyhow::bail!("frame too large");
        }
    }

    Ok(buf)
}

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
    /// Set LED effect on a device (by MAC address)
    SetLed {
        /// Device MAC address (e.g. "aa:bb:cc:dd:ee:ff")
        mac: String,
        #[command(subcommand)]
        effect: LedEffectCommand,
    },
}

#[derive(Subcommand)]
enum LedEffectCommand {
    /// Set a static color (e.g. "static ff00ff")
    Static {
        /// Hex color (e.g. "ff00ff")
        color: String,
    },
    /// Breathing effect — pulses a color (e.g. "breathing ff0000 --speed 3")
    Breathing {
        /// Hex color (e.g. "ff0000")
        color: String,
        /// Animation speed 0-4 (default 2)
        #[arg(long, default_value_t = 2)]
        speed: u8,
    },
    /// Rainbow effect — rotating hue across LEDs
    Rainbow {
        /// Animation speed 0-4 (default 2)
        #[arg(long, default_value_t = 2)]
        speed: u8,
    },
}

impl LedEffectCommand {
    fn into_led_effect(self) -> ipc::LedEffect {
        match self {
            LedEffectCommand::Static { color } => ipc::LedEffect::Static { color },
            LedEffectCommand::Breathing { color, speed } => {
                ipc::LedEffect::Breathing { color, speed }
            }
            LedEffectCommand::Rainbow { speed } => ipc::LedEffect::Rainbow { speed },
        }
    }
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
    // Summary line
    let wireless_fan_count: usize = status.wireless_fans.iter().map(|r| r.fans.len()).sum();
    println!(
        "Wireless Receivers: {}, Fans: {}",
        status.wireless_fans.len(),
        wireless_fan_count
    );
    println!("Wired Fans: {}", status.wired_lcds.len());

    // Wireless receivers + fans
    for (i, device) in status.wireless_fans.iter().enumerate() {
        println!();
        println!("Wireless Receiver #{i}");
        println!("  mac: {}", device.mac);

        for (j, f) in device.fans.iter().enumerate() {
            println!();
            println!("  Fan #{j}");

            // Find config for this fan's LCD — match by index into wireless_lcds
            // (wireless fans and LCDs pair by position)
            let lcd = status.wireless_lcds.get(i * device.fans.len() + j);
            if let Some(lcd) = lcd {
                if let Some(serial) = &lcd.serial {
                    println!("    serial: {serial}");
                }
            }

            println!("    rpm: {}", f.rpm);
            println!("    pwm: {}", f.pwm);

            if let Some(lcd) = lcd {
                if let Some(serial) = &lcd.serial {
                    let config = status.config_fans.iter().find(|c| c.serial == *serial);
                    if let Some(c) = config {
                        if let Some(v) = &c.video {
                            println!("    video: {} @ {}fps", v.path, v.fps);
                        }
                    }
                }
            }
        }
    }

    // Wired fans
    for (i, lcd) in status.wired_lcds.iter().enumerate() {
        println!();
        println!("Wired Fan #{i}");
        println!("  port: {}", lcd.port);
        println!("  index: {}", lcd.lcd_index);

        let config = status
            .config_fans
            .iter()
            .find(|c| c.port == Some(lcd.port) && c.lcd_index == Some(lcd.lcd_index));
        if let Some(c) = config {
            if let Some(v) = &c.video {
                println!("  video: {} @ {}fps", v.path, v.fps);
            }
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
                WirelessCommands::SetLed { mac, effect } => {
                    let led_effect = effect.into_led_effect();
                    if daemon_available() {
                        let req = Request::SetLed {
                            mac: mac.clone(),
                            effect: led_effect,
                        };
                        let resp = daemon_request(&req)?;
                        if resp.ok {
                            println!("OK — set LED on {mac}");
                        } else if let Some(err) = resp.error {
                            anyhow::bail!("{err}");
                        }
                    } else {
                        controller.init()?;
                        let rf_devices = controller.get_device_list()?;

                        let target = rf_devices
                            .iter()
                            .find(|d| slv3h::format_mac(&d.mac) == mac)
                            .ok_or_else(|| anyhow::anyhow!("Device {mac} not found"))?;

                        controller.set_led(target, &led_effect)?;
                        println!("Set LED on {mac}: {:?}", led_effect);
                    }
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

use std::io::Read;
use std::process::{Command, Stdio};

use clap::{Parser, Subcommand};
use unifand::device::DeviceKind;
use unifand::devices::slv3h::Slv3hController;
use unifand::devices::tl_fan::TlFanController;
use unifand::devices::tl_lcd_wired::TlLcdWired;
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
    SetSpeed {
        port: u8,
        fan: u8,
        pwm: u8,
    },
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
    Wireless(LcdTransport),
    Wired(TlLcdWired),
}

/// Convert any image to 400x400 JPG using ffmpeg.
fn convert_image_to_jpg(path: &str) -> anyhow::Result<Vec<u8>> {
    let output = Command::new("ffmpeg")
        .args([
            "-i", path,
            "-vf", "scale=400:400:force_original_aspect_ratio=increase,crop=400:400",
            "-frames:v", "1",
            "-f", "mjpeg",
            "-q:v", "5",
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
    // Try wireless LCD first (USB bulk)
    if let Ok(lcd) = LcdTransport::open() {
        println!("Found wireless LCD");
        return Ok(Lcd::Wireless(lcd));
    }

    // Try wired LCD (HID)
    let devices = unifand::discover()?;
    if let Some(lcd_info) = devices
        .iter()
        .find(|d| matches!(d.kind, DeviceKind::TlLcdWired))
    {
        let api = hidapi::HidApi::new()?;
        let lcd = TlLcdWired::open(&api, lcd_info)?;
        lcd.handshake()?;
        println!("Found wired LCD");
        return Ok(Lcd::Wired(lcd));
    }

    anyhow::bail!("No LCD display found (checked wireless USB and wired HID)")
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
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
            let lcd = open_lcd()?;

            match command {
                DisplayCommands::Reset => {
                    match &lcd {
                        Lcd::Wireless(w) => {
                            w.send_cmd_bare(LcdCmd::Reboot)?;
                            println!("Sent reboot command to display");
                        }
                        Lcd::Wired(_) => {
                            anyhow::bail!("Reset not supported on wired LCD");
                        }
                    }
                }
                DisplayCommands::Brightness { level } => {
                    match &lcd {
                        Lcd::Wireless(w) => {
                            w.send_cmd(LcdCmd::Brightness, level)?;
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
                DisplayCommands::Rotate { rotation } => {
                    match &lcd {
                        Lcd::Wireless(w) => {
                            if rotation > 3 {
                                anyhow::bail!("Rotation must be 0-3 (0°, 90°, 180°, 270°)");
                            }
                            w.send_cmd(LcdCmd::Rotate, rotation as u8)?;
                            println!("Rotated display to {}°", rotation * 90);
                        }
                        Lcd::Wired(w) => {
                            let rot = match rotation {
                                0 => ScreenRotation::Deg0,
                                90 => ScreenRotation::Deg90,
                                180 => ScreenRotation::Deg180,
                                270 => ScreenRotation::Deg270,
                                _ => anyhow::bail!("Invalid rotation: {rotation}. Use 0, 90, 180, or 270."),
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
                    }
                }
                DisplayCommands::Image { path } => {
                    let jpg_data = convert_image_to_jpg(&path)?;
                    match &lcd {
                        Lcd::Wireless(w) => {
                            w.push_jpg(&jpg_data)?;
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
                        anyhow::bail!("FPS too high (max 60) — the LCD can't keep up and will lock up");
                    }
                    if fps > 30 {
                        eprintln!("Warning: FPS above 30 may cause the LCD to lock up");
                    }

                    // Set frame rate (wireless only — wired doesn't have this command)
                    if let Lcd::Wireless(w) = &lcd {
                        w.send_cmd(LcdCmd::SetFrameRate, fps)?;
                    }

                    let frame_duration = std::time::Duration::from_millis(1000 / fps as u64);

                    loop {
                        let mut child = Command::new("ffmpeg")
                            .args([
                                "-i", &path,
                                "-vf", "scale=400:400:force_original_aspect_ratio=increase,crop=400:400",
                                "-r", &fps.to_string(),
                                "-f", "mjpeg",
                                "-q:v", "5",
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
                                        Lcd::Wireless(w) => w.push_jpg(&frame),
                                        Lcd::Wired(w) => w.send_sync_jpg(&frame),
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

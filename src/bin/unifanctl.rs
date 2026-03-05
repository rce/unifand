use clap::{Parser, Subcommand};
use unifand::device::DeviceKind;
use unifand::devices::tl_fan::TlFanController;
use unifand::devices::tl_lcd_wired::TlLcdWired;
use unifand::devices::tl_lcd_wireless::TlLcdWireless;
use unifand::protocol::lcd::{LcdControlSetting, LcdMode, ScreenRotation};

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
    /// Fan control commands
    Fan {
        #[command(subcommand)]
        command: FanCommands,
    },
    /// LCD display commands
    Lcd {
        #[command(subcommand)]
        command: LcdCommands,
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
enum LcdCommands {
    /// Set LCD brightness
    Brightness { level: u8 },
    /// Rotate LCD display
    Rotate { degrees: u16 },
    /// Display a JPEG image
    ShowImage { path: String },
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
                .find(|d| matches!(d.kind, DeviceKind::TlFanController | DeviceKind::Slv3h))
                .ok_or_else(|| anyhow::anyhow!("No fan controller found"))?;
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
        Commands::Lcd { command } => {
            let devices = unifand::discover()?;
            let lcd_info = devices
                .iter()
                .find(|d| matches!(d.kind, DeviceKind::TlLcdWired | DeviceKind::TlLcdWireless | DeviceKind::Slv3h))
                .ok_or_else(|| anyhow::anyhow!("No LCD device found"))?;

            let api = hidapi::HidApi::new()?;

            match &lcd_info.kind {
                DeviceKind::TlLcdWired => {
                    let lcd = TlLcdWired::open(&api, lcd_info)?;
                    match command {
                        LcdCommands::Brightness { level } => {
                            lcd.set_control(&LcdControlSetting {
                                mode: LcdMode::LcdSetting,
                                jpg_index: 0,
                                brightness: level,
                                video_fps: 0,
                                rotation: ScreenRotation::Deg0,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                            println!("Set LCD brightness to {level}");
                        }
                        LcdCommands::Rotate { degrees } => {
                            let rotation = match degrees {
                                0 => ScreenRotation::Deg0,
                                90 => ScreenRotation::Deg90,
                                180 => ScreenRotation::Deg180,
                                270 => ScreenRotation::Deg270,
                                _ => anyhow::bail!("Invalid rotation: {degrees}. Use 0, 90, 180, or 270."),
                            };
                            lcd.set_control(&LcdControlSetting {
                                mode: LcdMode::LcdSetting,
                                jpg_index: 0,
                                brightness: 100,
                                video_fps: 0,
                                rotation,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                            println!("Rotated LCD to {degrees} degrees");
                        }
                        LcdCommands::ShowImage { path } => {
                            let jpg_data = std::fs::read(&path)?;
                            lcd.send_jpg(&jpg_data)?;
                            println!("Sent image {path} to LCD");
                        }
                    }
                }
                DeviceKind::TlLcdWireless | DeviceKind::Slv3h => {
                    let lcd = TlLcdWireless::open(&api, lcd_info)?;
                    match command {
                        LcdCommands::Brightness { level } => {
                            lcd.set_control(&LcdControlSetting {
                                mode: LcdMode::LcdSetting,
                                jpg_index: 0,
                                brightness: level,
                                video_fps: 0,
                                rotation: ScreenRotation::Deg0,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                            println!("Set LCD brightness to {level}");
                        }
                        LcdCommands::Rotate { degrees } => {
                            let rotation = match degrees {
                                0 => ScreenRotation::Deg0,
                                90 => ScreenRotation::Deg90,
                                180 => ScreenRotation::Deg180,
                                270 => ScreenRotation::Deg270,
                                _ => anyhow::bail!("Invalid rotation: {degrees}. Use 0, 90, 180, or 270."),
                            };
                            lcd.set_control(&LcdControlSetting {
                                mode: LcdMode::LcdSetting,
                                jpg_index: 0,
                                brightness: 100,
                                video_fps: 0,
                                rotation,
                                enable_test: false,
                                test_color: (0, 0, 0),
                            })?;
                            println!("Rotated LCD to {degrees} degrees");
                        }
                        LcdCommands::ShowImage { .. } => {
                            eprintln!("Error: show-image is not supported on wireless LCD (requires USB bulk transfer)");
                            std::process::exit(1);
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    Ok(())
}

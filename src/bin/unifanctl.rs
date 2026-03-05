use clap::{Parser, Subcommand};
use unifand::device::DeviceKind;
use unifand::devices::slv3h::Slv3hController;
use unifand::devices::tl_fan::TlFanController;
use unifand::devices::tl_lcd_wired::TlLcdWired;
use unifand::devices::tl_lcd_wireless::TlLcdWireless;
use unifand::protocol::lcd::{LcdControlSetting, LcdMode, ScreenRotation};
use unifand::protocol::slv3h;

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

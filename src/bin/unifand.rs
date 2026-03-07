use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use unifand::device::DeviceKind;
use unifand::devices::slv3h::Slv3hController;
use unifand::devices::tl_lcd_wired::TlLcdWired;
use unifand::ipc::{
    self, Config, DaemonStatus, DisplayState, FanReading, Request, Response, WirelessFanState,
};
use unifand::protocol::lcd::{LcdControlSetting, LcdMode, ScreenRotation};
use unifand::protocol::slv3h;
use unifand::transport::lcd::{LcdCmd, LcdTransport};

/// A wired LCD with its chain position.
struct WiredLcd {
    lcd: TlLcdWired,
    port: u8,
    lcd_index: u8,
}

/// Which LCD type the daemon owns.
enum Lcd {
    Wireless(Vec<LcdTransport>),
    Wired(Vec<WiredLcd>),
    None,
}

struct DaemonState {
    start_time: Instant,
    display: DisplayState,
    lcd: Lcd,
    video_cancel: Option<std::sync::mpsc::Sender<()>>,
    config_fans: Vec<ipc::FanConfig>,
}

impl DaemonState {
    fn stop_video(&mut self) {
        if let Some(tx) = self.video_cancel.take() {
            let _ = tx.send(());
        }
        // Only reset display state if we were actually playing video
        if self.display.mode == "video" {
            self.display = DisplayState::default();
        }
    }
}

fn open_lcd() -> Lcd {
    // Try wireless LCDs first
    if let Ok(lcds) = LcdTransport::open_all() {
        if !lcds.is_empty() {
            eprintln!("Found {} wireless LCD(s)", lcds.len());
            for (i, lcd) in lcds.iter().enumerate() {
                eprintln!("  LCD {i}: serial={:?}", lcd.serial);
            }
            return Lcd::Wireless(lcds);
        }
    }

    // Try wired LCDs (there may be multiple daisy-chained)
    if let Ok(devices) = unifand::discover() {
        let lcd_infos: Vec<_> = devices
            .iter()
            .filter(|d| matches!(d.kind, DeviceKind::TlLcdWired))
            .collect();

        if !lcd_infos.is_empty() {
            if let Ok(api) = hidapi::HidApi::new() {
                let mut wired_lcds = Vec::new();
                for lcd_info in &lcd_infos {
                    let Ok(lcd) = TlLcdWired::open(&api, lcd_info) else {
                        eprintln!("Failed to open wired LCD at {:?}", lcd_info.path);
                        continue;
                    };
                    match lcd.handshake() {
                        Ok(info) => {
                            eprintln!(
                                "Wired LCD handshake OK: mode={}, frame_index={}",
                                info.mode, info.frame_index
                            );
                        }
                        Err(e) => {
                            eprintln!("Wired LCD handshake failed: {e}");
                            continue;
                        }
                    }
                    let (port, lcd_index) = match lcd.read_serial_number() {
                        Ok(sn) => {
                            eprintln!(
                                "Wired LCD: serial={:?} port={} index={}",
                                sn.serial, sn.port, sn.lcd_index
                            );
                            (sn.port, sn.lcd_index)
                        }
                        Err(e) => {
                            eprintln!("Wired LCD read_serial_number failed: {e}, using defaults");
                            (0, wired_lcds.len() as u8)
                        }
                    };
                    wired_lcds.push(WiredLcd {
                        lcd,
                        port,
                        lcd_index,
                    });
                }
                if !wired_lcds.is_empty() {
                    eprintln!("Found {} wired LCD(s)", wired_lcds.len());
                    return Lcd::Wired(wired_lcds);
                }
            }
        }
    }

    eprintln!("No LCD found");
    Lcd::None
}

fn get_wireless_fan_status() -> Vec<WirelessFanState> {
    let Ok(api) = hidapi::HidApi::new() else {
        return vec![];
    };
    let Ok(devices) = unifand::discover() else {
        return vec![];
    };
    let Some(hub_info) = devices.iter().find(|d| matches!(d.kind, DeviceKind::Slv3h)) else {
        return vec![];
    };

    let Ok(mut controller) = Slv3hController::open(&api, hub_info) else {
        return vec![];
    };
    if controller.init().is_err() {
        return vec![];
    }
    let Ok(rf_devices) = controller.get_device_list() else {
        return vec![];
    };

    rf_devices
        .iter()
        .map(|dev| WirelessFanState {
            mac: slv3h::format_mac(&dev.mac),
            fan_count: dev.fan_num,
            fans: (0..dev.fan_num as usize)
                .filter(|&i| i < 4)
                .map(|i| FanReading {
                    rpm: dev.fan_speeds[i],
                    pwm: dev.fan_pwm[i],
                })
                .collect(),
        })
        .collect()
}

fn handle_request(state: &Arc<Mutex<DaemonState>>, req: Request) -> Response {
    // Log incoming requests (skip Status to avoid spam)
    match &req {
        Request::Status => {}
        other => eprintln!("IPC request: {other:?}"),
    }
    let resp = handle_request_inner(state, req);
    if !resp.ok {
        if let Some(err) = &resp.error {
            eprintln!("IPC error: {err}");
        }
    }
    resp
}

fn handle_request_inner(state: &Arc<Mutex<DaemonState>>, req: Request) -> Response {
    match req {
        Request::Status => {
            let st = state.lock().unwrap();
            let wireless_fans = get_wireless_fan_status();
            Response::with_status(DaemonStatus {
                uptime_secs: st.start_time.elapsed().as_secs(),
                wireless_lcds: match &st.lcd {
                    Lcd::Wireless(v) => v
                        .iter()
                        .map(|l| ipc::WirelessLcdState {
                            serial: l.serial.clone(),
                        })
                        .collect(),
                    _ => vec![],
                },
                wired_lcds: match &st.lcd {
                    Lcd::Wired(v) => v
                        .iter()
                        .map(|w| ipc::WiredLcdState {
                            port: w.port,
                            lcd_index: w.lcd_index,
                        })
                        .collect(),
                    _ => vec![],
                },
                display: st.display.clone(),
                wireless_fans,
                config_fans: st.config_fans.clone(),
            })
        }
        Request::DisplayImage { path } => {
            let mut st = state.lock().unwrap();
            st.stop_video();

            let jpg_data = match convert_image_to_jpg(&path) {
                Ok(d) => d,
                Err(e) => return Response::err(format!("ffmpeg: {e}")),
            };

            let result = match &st.lcd {
                Lcd::Wireless(screens) => {
                    let mut r = Ok(());
                    for w in screens {
                        if let Err(e) = w.push_jpg(&jpg_data) {
                            r = Err(e);
                        }
                    }
                    r
                }
                Lcd::Wired(wired) => {
                    let mut r = Ok(());
                    for w in wired {
                        if let Err(e) = w.lcd.send_jpg(&jpg_data) {
                            r = Err(e);
                        } else if let Err(e) = w.lcd.set_control(&LcdControlSetting {
                            mode: LcdMode::ShowJpg,
                            jpg_index: 0,
                            brightness: 100,
                            video_fps: 0,
                            rotation: ScreenRotation::Deg0,
                            enable_test: false,
                            test_color: (0, 0, 0),
                        }) {
                            r = Err(e);
                        }
                    }
                    r
                }
                Lcd::None => return Response::err("no LCD connected"),
            };

            match result {
                Ok(()) => {
                    st.display = DisplayState {
                        mode: "image".into(),
                        source: Some(path),
                        fps: None,
                        looping: false,
                    };
                    Response::ok()
                }
                Err(e) => Response::err(e.to_string()),
            }
        }
        Request::DisplayVideo {
            path,
            fps,
            loop_video,
        } => {
            if fps > 60 {
                return Response::err("FPS too high (max 60)");
            }

            let mut st = state.lock().unwrap();
            st.stop_video();

            // Set frame rate on LCD
            let setup_result = match &st.lcd {
                Lcd::Wireless(screens) => {
                    let mut r = Ok(());
                    for w in screens {
                        if let Err(e) = w.send_cmd(LcdCmd::SetFrameRate, fps) {
                            r = Err(e);
                        }
                    }
                    r
                }
                Lcd::Wired(wired) => {
                    let mut r = Ok(());
                    for w in wired {
                        if let Err(e) = w.lcd.set_control(&LcdControlSetting {
                            mode: LcdMode::ShowJpg,
                            jpg_index: 0,
                            brightness: 100,
                            video_fps: fps,
                            rotation: ScreenRotation::Deg0,
                            enable_test: false,
                            test_color: (0, 0, 0),
                        }) {
                            r = Err(e);
                        }
                    }
                    r
                }
                Lcd::None => return Response::err("no LCD connected"),
            };

            if let Err(e) = setup_result {
                return Response::err(e.to_string());
            }

            st.display = DisplayState {
                mode: "video".into(),
                source: Some(path.clone()),
                fps: Some(fps),
                looping: loop_video,
            };

            let (cancel_tx, cancel_rx) = std::sync::mpsc::channel();
            st.video_cancel = Some(cancel_tx);

            let state_clone = Arc::clone(state);
            std::thread::spawn(move || {
                video_loop(&state_clone, &path, fps, loop_video, cancel_rx);
            });

            Response::ok()
        }
        Request::DisplayBrightness { level } => {
            let st = state.lock().unwrap();
            let result = match &st.lcd {
                Lcd::Wireless(screens) => {
                    let mut r = Ok(());
                    for w in screens {
                        if let Err(e) = w.send_cmd(LcdCmd::Brightness, level) {
                            r = Err(e);
                        }
                    }
                    r
                }
                Lcd::Wired(wired) => {
                    let mut r = Ok(());
                    for w in wired {
                        if let Err(e) = w.lcd.set_control(&LcdControlSetting {
                            mode: LcdMode::LcdSetting,
                            jpg_index: 0,
                            brightness: level,
                            video_fps: 0,
                            rotation: ScreenRotation::Deg0,
                            enable_test: false,
                            test_color: (0, 0, 0),
                        }) {
                            r = Err(e);
                        }
                    }
                    r
                }
                Lcd::None => return Response::err("no LCD connected"),
            };
            match result {
                Ok(()) => Response::ok(),
                Err(e) => Response::err(e.to_string()),
            }
        }
        Request::DisplayRotate { rotation } => {
            let st = state.lock().unwrap();
            let result = match &st.lcd {
                Lcd::Wireless(screens) => {
                    if rotation > 3 {
                        return Response::err("rotation must be 0-3");
                    }
                    let mut r = Ok(());
                    for w in screens {
                        if let Err(e) = w.send_cmd(LcdCmd::Rotate, rotation as u8) {
                            r = Err(e);
                        }
                    }
                    r
                }
                Lcd::Wired(wired) => {
                    let rot = match rotation {
                        0 => ScreenRotation::Deg0,
                        90 => ScreenRotation::Deg90,
                        180 => ScreenRotation::Deg180,
                        270 => ScreenRotation::Deg270,
                        _ => return Response::err("invalid rotation: use 0, 90, 180, or 270"),
                    };
                    let mut r = Ok(());
                    for w in wired {
                        if let Err(e) = w.lcd.set_control(&LcdControlSetting {
                            mode: LcdMode::LcdSetting,
                            jpg_index: 0,
                            brightness: 100,
                            video_fps: 0,
                            rotation: rot,
                            enable_test: false,
                            test_color: (0, 0, 0),
                        }) {
                            r = Err(e);
                        }
                    }
                    r
                }
                Lcd::None => return Response::err("no LCD connected"),
            };
            match result {
                Ok(()) => Response::ok(),
                Err(e) => Response::err(e.to_string()),
            }
        }
        Request::DisplayReset => {
            let mut st = state.lock().unwrap();
            st.stop_video();
            let result = match &st.lcd {
                Lcd::Wireless(screens) => {
                    let mut r = Ok(());
                    for w in screens {
                        if let Err(e) = w.send_cmd_bare(LcdCmd::Reboot) {
                            r = Err(e);
                        }
                    }
                    r
                }
                Lcd::Wired(_) => return Response::err("reset not supported on wired LCDs"),
                Lcd::None => return Response::err("no LCD connected"),
            };
            match result {
                Ok(()) => {
                    st.display = DisplayState::default();
                    Response::ok()
                }
                Err(e) => Response::err(e.to_string()),
            }
        }
        Request::SetLed { mac, effect } => {
            let Ok(api) = hidapi::HidApi::new() else {
                return Response::err("failed to open HID API");
            };
            let Ok(devices) = unifand::discover() else {
                return Response::err("failed to discover devices");
            };
            let Some(hub_info) = devices.iter().find(|d| matches!(d.kind, DeviceKind::Slv3h))
            else {
                return Response::err("no SLV3H wireless hub found");
            };

            let Ok(mut controller) = Slv3hController::open(&api, hub_info) else {
                return Response::err("failed to open SLV3H controller");
            };
            if let Err(e) = controller.init() {
                return Response::err(format!("SLV3H init failed: {e}"));
            }
            let Ok(rf_devices) = controller.get_device_list() else {
                return Response::err("failed to get device list");
            };

            let Some(target) = rf_devices.iter().find(|d| slv3h::format_mac(&d.mac) == mac) else {
                return Response::err(format!("device {mac} not found"));
            };

            match controller.set_led(target, &effect) {
                Ok(()) => Response::ok(),
                Err(e) => Response::err(e.to_string()),
            }
        }
        Request::SetFanSpeed { mac, pwm } => {
            let Ok(api) = hidapi::HidApi::new() else {
                return Response::err("failed to open HID API");
            };
            let Ok(devices) = unifand::discover() else {
                return Response::err("failed to discover devices");
            };
            let Some(hub_info) = devices.iter().find(|d| matches!(d.kind, DeviceKind::Slv3h))
            else {
                return Response::err("no SLV3H wireless hub found");
            };

            let Ok(mut controller) = Slv3hController::open(&api, hub_info) else {
                return Response::err("failed to open SLV3H controller");
            };
            if let Err(e) = controller.init() {
                return Response::err(format!("SLV3H init failed: {e}"));
            }
            let Ok(rf_devices) = controller.get_device_list() else {
                return Response::err("failed to get device list");
            };

            let Some(target) = rf_devices.iter().find(|d| slv3h::format_mac(&d.mac) == mac) else {
                return Response::err(format!("device {mac} not found"));
            };

            let pwm_all = [pwm; 4];
            match controller.set_fan_pwm(target, &pwm_all) {
                Ok(()) => Response::ok(),
                Err(e) => Response::err(e.to_string()),
            }
        }
    }
}

fn video_loop(
    state: &Arc<Mutex<DaemonState>>,
    path: &str,
    fps: u8,
    loop_video: bool,
    cancel: std::sync::mpsc::Receiver<()>,
) {
    let frame_duration = std::time::Duration::from_millis(1000 / fps as u64);

    loop {
        let mut child = match Command::new("ffmpeg")
            .args([
                "-i",
                path,
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
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to spawn ffmpeg: {e}");
                break;
            }
        };

        let stdout = child.stdout.take().unwrap();
        let mut reader = std::io::BufReader::new(stdout);
        let mut frame_count: u64 = 0;

        loop {
            // Check for cancellation
            if cancel.try_recv().is_ok() {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }

            let frame_start = Instant::now();
            match read_jpeg_frame(&mut reader) {
                Ok(frame) => {
                    let st = state.lock().unwrap();
                    let result = match &st.lcd {
                        Lcd::Wireless(screens) => {
                            let mut r = Ok(());
                            for w in screens {
                                if let Err(e) = w.push_jpg(&frame) {
                                    r = Err(e);
                                }
                            }
                            r
                        }
                        Lcd::Wired(wired) => {
                            let mut r = Ok(());
                            for w in wired {
                                let res = if frame_count == 0 {
                                    w.lcd.send_jpg(&frame)
                                } else {
                                    w.lcd.send_sync_jpg(&frame)
                                };
                                if let Err(e) = res {
                                    r = Err(e);
                                }
                            }
                            r
                        }
                        Lcd::None => break,
                    };
                    drop(st);

                    if let Err(e) = result {
                        eprintln!("Error pushing frame: {e}");
                        break;
                    }
                    frame_count += 1;

                    let elapsed = frame_start.elapsed();
                    if elapsed < frame_duration {
                        std::thread::sleep(frame_duration - elapsed);
                    }
                }
                Err(_) => break,
            }
        }

        let _ = child.wait();

        if !loop_video {
            break;
        }

        // Check cancel before looping
        if cancel.try_recv().is_ok() {
            return;
        }
    }

    // Video ended naturally — update state
    let mut st = state.lock().unwrap();
    st.display = DisplayState::default();
    st.video_cancel = None;
}

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

fn read_jpeg_frame<R: std::io::Read>(reader: &mut R) -> anyhow::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(32768);
    let mut byte = [0u8; 1];

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

fn handle_client(stream: UnixStream, state: &Arc<Mutex<DaemonState>>) {
    let reader = BufReader::new(&stream);
    let mut writer = &stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Request>(&line) {
            Ok(req) => handle_request(state, req),
            Err(e) => Response::err(format!("invalid request: {e}")),
        };

        let json = serde_json::to_string(&response).unwrap();
        if writeln!(writer, "{json}").is_err() {
            break;
        }
    }
}

/// Cleans up the socket file on drop.
struct SocketGuard(std::path::PathBuf);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn reload_config(state: &Arc<Mutex<DaemonState>>, config_path: &Path) {
    let contents = match std::fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read {}: {e}", config_path.display());
            return;
        }
    };
    let config: Config = match serde_json::from_str(&contents) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to parse {}: {e}", config_path.display());
            return;
        }
    };

    state.lock().unwrap().stop_video();
    state.lock().unwrap().config_fans = config.fans.clone();
    apply_config(state, &config);
    eprintln!("Config reloaded from {}", config_path.display());
}

fn watch_config(state: Arc<Mutex<DaemonState>>, config_path: std::path::PathBuf) {
    use notify::{EventKind, RecursiveMode, Watcher};

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res: Result<notify::Event, _>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Failed to create file watcher: {e}");
            return;
        }
    };

    let watch_dir = match config_path.parent() {
        Some(p) => p,
        None => {
            eprintln!("Config path has no parent directory");
            return;
        }
    };

    if let Err(e) = watcher.watch(watch_dir, RecursiveMode::NonRecursive) {
        eprintln!("Failed to watch {}: {e}", watch_dir.display());
        return;
    }

    eprintln!("Watching {} for changes", config_path.display());
    let config_filename = config_path.file_name().unwrap().to_owned();

    loop {
        match rx.recv() {
            Ok(event) => {
                let dominated = event
                    .paths
                    .iter()
                    .any(|p| p.file_name().map(|f| f == config_filename).unwrap_or(false));
                if !dominated {
                    continue;
                }
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        // Debounce: drain any additional events within 200ms
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        while rx.try_recv().is_ok() {}
                        reload_config(&state, &config_path);
                    }
                    _ => {}
                }
            }
            Err(_) => break,
        }
    }
}

fn apply_config(state: &Arc<Mutex<DaemonState>>, config: &Config) {
    let st = state.lock().unwrap();
    let is_wired = matches!(&st.lcd, Lcd::Wired(_));
    drop(st);

    if is_wired {
        apply_config_wired(state, config);
    } else {
        apply_config_wireless(state, config);
    }
}

fn apply_config_wireless(state: &Arc<Mutex<DaemonState>>, config: &Config) {
    let lcd_serials: Vec<String> = {
        let st = state.lock().unwrap();
        match &st.lcd {
            Lcd::Wireless(lcds) => lcds.iter().filter_map(|l| l.serial.clone()).collect(),
            _ => vec![],
        }
    };

    for fan in &config.fans {
        if !fan.serial.is_empty() && !lcd_serials.iter().any(|s| s == &fan.serial) {
            eprintln!(
                "Config: serial {:?} not found among connected LCDs",
                fan.serial
            );
        }
    }

    for serial in &lcd_serials {
        if !config.fans.iter().any(|f| f.serial == *serial) {
            eprintln!("Config: LCD {serial} has no config entry");
        }
    }

    // Apply LED configs for wireless fans (by MAC)
    for fan in &config.fans {
        if let (Some(effect), Some(mac)) = (&fan.led, &fan.mac) {
            eprintln!("Config: setting LED on {} — {:?}", mac, effect);
            let req = Request::SetLed {
                mac: mac.clone(),
                effect: effect.clone(),
            };
            let resp = handle_request(state, req);
            if !resp.ok {
                eprintln!("Config: failed to apply LED: {:?}", resp.error);
            }
        }
    }

    for fan in &config.fans {
        if fan.serial.is_empty() {
            eprintln!("Config: skipping fan entry with empty serial");
            continue;
        }
        if !lcd_serials.iter().any(|s| s == &fan.serial) {
            continue;
        }
        if let Some(video) = &fan.video {
            eprintln!(
                "Config: playing {} on {} (fps={})",
                video.path, fan.serial, video.fps
            );
            let req = Request::DisplayVideo {
                path: video.path.clone(),
                fps: video.fps,
                loop_video: true,
            };
            let resp = handle_request(state, req);
            if !resp.ok {
                eprintln!("Config: failed to apply video: {:?}", resp.error);
            }
            return;
        }
    }
}

fn apply_config_wired(state: &Arc<Mutex<DaemonState>>, config: &Config) {
    // For wired LCDs, match by port+lcd_index
    let wired_positions: Vec<(u8, u8)> = {
        let st = state.lock().unwrap();
        match &st.lcd {
            Lcd::Wired(wired) => wired.iter().map(|w| (w.port, w.lcd_index)).collect(),
            _ => return,
        }
    };

    for fan in &config.fans {
        let Some(port) = fan.port else {
            eprintln!("Config: skipping wired fan entry without port");
            continue;
        };
        let Some(lcd_index) = fan.lcd_index else {
            eprintln!("Config: skipping wired fan entry without lcd_index");
            continue;
        };

        if !wired_positions
            .iter()
            .any(|&(p, i)| p == port && i == lcd_index)
        {
            eprintln!(
                "Config: wired LCD port={} index={} not found",
                port, lcd_index
            );
            continue;
        }

        if let Some(video) = &fan.video {
            eprintln!(
                "Config: playing {} on wired LCD port={} index={} (fps={})",
                video.path, port, lcd_index, video.fps
            );
            // For now, apply video to all wired LCDs via the shared DisplayVideo path
            // TODO: per-LCD video requires separate video loops
            let req = Request::DisplayVideo {
                path: video.path.clone(),
                fps: video.fps,
                loop_video: true,
            };
            let resp = handle_request(state, req);
            if !resp.ok {
                eprintln!("Config: failed to apply video: {:?}", resp.error);
            }
            return;
        }
    }
}

fn main() -> anyhow::Result<()> {
    let sock_path = ipc::socket_path();

    // Remove stale socket
    if sock_path.exists() {
        std::fs::remove_file(&sock_path)?;
    }

    // Open hardware
    let lcd = open_lcd();

    let state = Arc::new(Mutex::new(DaemonState {
        start_time: Instant::now(),
        display: DisplayState::default(),
        lcd,
        video_cancel: None,
        config_fans: vec![],
    }));

    // Load and apply initial config
    let config_path = ipc::config_path();
    if config_path.exists() {
        reload_config(&state, &config_path);
    }

    let listener = UnixListener::bind(&sock_path)?;
    let _guard = SocketGuard(sock_path.clone());
    eprintln!("unifand listening on {}", sock_path.display());

    // Watch config file for changes
    {
        let state_clone = Arc::clone(&state);
        let config_path = config_path.clone();
        std::thread::spawn(move || {
            watch_config(state_clone, config_path);
        });
    }

    // Register signal handler that calls exit() so Drop guards run
    {
        extern "C" fn on_signal(_: i32) {
            std::process::exit(0);
        }
        unsafe extern "C" {
            safe fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
        }
        signal(2, on_signal); // SIGINT
        signal(15, on_signal); // SIGTERM
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    handle_client(stream, &state);
                });
            }
            Err(e) => {
                eprintln!("Accept error: {e}");
            }
        }
    }

    Ok(())
}

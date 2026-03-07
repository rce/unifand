/// SLV3H wireless hub controller.
///
/// Coordinates the SLV3H HID hub, RF TX dongle, and RF RX dongle
/// to control wireless Lian Li fans.
use std::time::Duration;

use hidapi::HidApi;

use crate::Result;
use crate::device::DeviceInfo;
use crate::protocol::slv3h::{self, MasterInfo, RfDeviceInfo};
use crate::transport::hid::HidTransport;
use crate::transport::rf::RfTransport;

pub struct Slv3hController {
    hub: HidTransport,
    tx: RfTransport,
    rx: RfTransport,
    master_info: Option<MasterInfo>,
}

impl Slv3hController {
    /// Open all three devices: SLV3H hub (HID), TX dongle, RX dongle.
    pub fn open(api: &HidApi, hub_info: &DeviceInfo) -> Result<Self> {
        let hub = HidTransport::open_path(api, &hub_info.path)?;
        let tx = RfTransport::open_tx()?;
        let rx = RfTransport::open_rx()?;
        Ok(Self {
            hub,
            tx,
            rx,
            master_info: None,
        })
    }

    /// Get the hub's MAC address via HID command.
    pub fn get_hub_mac(&self) -> Result<[u8; 6]> {
        let pkt = slv3h::get_mac_packet();
        // SLV3H uses Report ID 0, so we prepend 0x00 for hidapi
        let mut buf = [0u8; 65];
        buf[0] = 0x00; // Report ID 0
        buf[1..65].copy_from_slice(&pkt);
        self.hub.raw_write(&buf)?;

        let resp = self.hub.raw_read(64, 100)?;
        slv3h::parse_mac_response(&resp)
            .ok_or_else(|| crate::Error::InvalidResponse("failed to get hub MAC".into()))
    }

    /// Query the master MAC, firmware version, and system clock from the TX dongle.
    pub fn query_master(&mut self, channel: u8) -> Result<MasterInfo> {
        let pkt = slv3h::query_master_packet(channel);
        let resp = self.tx.write_read(&pkt)?;
        let info = slv3h::parse_master_response(&resp)
            .ok_or_else(|| crate::Error::InvalidResponse("failed to query master".into()))?;
        self.master_info = Some(info.clone());
        Ok(info)
    }

    /// Initialize: get hub MAC and query master.
    pub fn init(&mut self) -> Result<MasterInfo> {
        let _hub_mac = self.get_hub_mac()?;
        self.query_master(slv3h::DEFAULT_CHANNEL)
    }

    /// Get the list of RF devices (fans, strimers, etc.) from the RX dongle.
    pub fn get_device_list(&self) -> Result<Vec<RfDeviceInfo>> {
        let master_mac = self.master_info.as_ref().map(|m| m.mac).unwrap_or([0u8; 6]);

        let page_count: u8 = 1;
        let pkt = slv3h::get_device_list_packet(page_count);
        self.rx.write(&pkt)?;

        let read_len = slv3h::PAGE_LENGTH * page_count as usize;
        let resp = self.rx.read_large(read_len, Duration::from_millis(1000))?;

        Ok(slv3h::parse_device_list(&resp, &master_mac))
    }

    /// Set fan PWM values for a specific device.
    pub fn set_fan_pwm(&self, device: &RfDeviceInfo, pwm: &[u8; 4]) -> Result<()> {
        let master_mac = self.master_info.as_ref().map(|m| m.mac).ok_or_else(|| {
            crate::Error::InvalidResponse("not initialized — call init() first".into())
        })?;

        let channel = self
            .master_info
            .as_ref()
            .map(|_| slv3h::DEFAULT_CHANNEL)
            .unwrap();

        let rf = slv3h::build_rf_bind_packet(
            &device.mac,
            &master_mac,
            device.rx_type,
            channel,
            1, // slave_index > 0 means bound
            pwm,
        );
        let chunks = slv3h::fragment_rf_packet(&rf, device.channel, device.rx_type);
        for chunk in &chunks {
            self.tx.write(chunk)?;
        }
        Ok(())
    }

    /// Set LED effect for a specific device via RF_RGB_SYNC.
    ///
    /// Renders a static color effect, compresses it with tinyuz,
    /// and sends the multi-packet RF_RGB_SYNC sequence.
    pub fn set_led(&self, device: &RfDeviceInfo, colors: &[(u8, u8, u8)]) -> Result<()> {
        use crate::protocol::tinyuz;

        let master_mac = self.master_info.as_ref().map(|m| m.mac).ok_or_else(|| {
            crate::Error::InvalidResponse("not initialized — call init() first".into())
        })?;

        let fan_num = device.fan_num.max(1);
        let led_num = (slv3h::LEDS_PER_FAN * fan_num as usize) as u8;

        // Render static color frames
        let rgb_data = slv3h::render_static_rgb(colors, fan_num);

        // Compress with tinyuz (4KB dictionary, matching L-Connect)
        let compressed = tinyuz::tuz_compress(&rgb_data, 4096);

        eprintln!(
            "LED: {} LEDs, {} frames, {} bytes raw → {} bytes compressed",
            led_num,
            slv3h::STATIC_FRAMES,
            rgb_data.len(),
            compressed.len()
        );

        // Generate a simple effect_index (incremented to trigger update)
        let effect_index = [0x01, 0x00, 0x00, 0x00];

        // Build multi-packet RF_RGB_SYNC sequence
        let rf_packets = slv3h::build_rf_rgb_sync_packets(
            &device.mac,
            &master_mac,
            &effect_index,
            &compressed,
            slv3h::STATIC_FRAMES as u16,
            led_num,
            60.0, // ~16fps interval (matches L-Connect pcap)
        );

        // Send each RF packet (fragmented into USB chunks)
        for (i, rf) in rf_packets.iter().enumerate() {
            let chunks = slv3h::fragment_rf_packet(rf, device.channel, device.rx_type);
            for chunk in &chunks {
                self.tx.write(chunk)?;
            }
            // Metadata packet (index 0) is sent 4 times with 20ms delay (per L-Connect)
            if i == 0 {
                for _ in 0..3 {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    let chunks = slv3h::fragment_rf_packet(rf, device.channel, device.rx_type);
                    for chunk in &chunks {
                        self.tx.write(chunk)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Save configuration to all bound devices (broadcast).
    pub fn save_config(&self) -> Result<()> {
        let master_mac = self
            .master_info
            .as_ref()
            .map(|m| m.mac)
            .ok_or_else(|| crate::Error::InvalidResponse("not initialized".into()))?;

        let channel = slv3h::DEFAULT_CHANNEL;
        let rf = slv3h::build_rf_save_config_packet(&master_mac);
        let chunks = slv3h::fragment_rf_packet(&rf, channel, 0xFF);
        for chunk in &chunks {
            self.tx.write(chunk)?;
        }
        Ok(())
    }

    /// Get master info (must call init() first).
    pub fn master_info(&self) -> Option<&MasterInfo> {
        self.master_info.as_ref()
    }
}

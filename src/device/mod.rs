pub mod sdr;

use std::{
    collections::HashMap,
    path::Path,
    sync::{LazyLock, Mutex},
};

use anyhow::Context;
use soapysdr::Device as RawDevice;

use sdr::SDRConfig;

pub struct Device {
    pub raw: RawDevice,
    pub config: SDRConfig,
    pub running: std::sync::Arc<Mutex<bool>>,
}

impl Device {
    pub fn new(raw: RawDevice, config: SDRConfig) -> Self {
        Self {
            raw,
            config,
            running: std::sync::Arc::new(Mutex::new(false)),
        }
    }
}

pub mod config {
    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    pub enum Device {
        HackRF {
            // plugin: SoapyHackRF(patched)
            // direction: "Rx" | "Tx" | "RxTx",
            direction: String,

            // freq: MHz
            freq_mhz: usize,

            // serial: ex) 0000000000000000f77c60dc259132c3
            // `hackrf_info` to get serial
            serial: String,
        },
        Virtual {
            // plugin: soapy-utils/soapy-virtual
            // direction: "Rx" | "Tx" | "RxTx",
            direction: String,
        },
        File {
            // plugin: soapy-utils/soapy-file
            // direction: "Rx"
            direction: String,

            // path: file path
            path: String,
        },
    }

    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    pub struct List {
        pub devices: Vec<Device>,
    }
}

static INTERNAL_DEVICE_INFO: LazyLock<HashMap<&str, (&str, &str)>> = LazyLock::new(|| {
    // [driver_name] => (driver_name, plugin_path)

    let mut hm = HashMap::new();

    hm.insert("hackrf", ("hackrf", "SoapyHackRF"));
    hm.insert("virtual", ("virtual", "soapy-utils/soapy-virtual"));
    hm.insert("file", ("file", "soapy-utils/soapy-file"));

    hm
});

const NUM_CHANNELS: usize = 16usize;

fn open_hackrf(config: config::Device) -> anyhow::Result<(Option<Device>, Option<Device>)> {
    let (driver, plugin_path) = INTERNAL_DEVICE_INFO.get("hackrf").unwrap();
    let config::Device::HackRF {
        direction,
        freq_mhz,
        serial,
    } = config
    else {
        return Err(anyhow::anyhow!("Invalid config"));
    };

    log::trace!(
        "driver: {}, plugin_path: {}, serial: {}",
        driver,
        plugin_path,
        serial
    );

    let dev = RawDevice::new(format!("driver={},serial={}", driver, serial).as_str())
        .context("failed to open device")?;

    let sdr_config = SDRConfig {
        channels: 0,
        num_channels: NUM_CHANNELS,
        center_freq: freq_mhz as f64 * 1.0e6,
        freq_mhz,
        sample_rate: NUM_CHANNELS as f64 * 1.0e6,
        bandwidth: NUM_CHANNELS as f64 * 1.0e6,
        gain: 64.,
    };

    sdr_config.set(&dev)?;

    match direction.as_str() {
        "Rx" => Ok((Some(Device::new(dev, sdr_config)), None)),
        "Tx" => Ok((None, Some(Device::new(dev, sdr_config)))),
        "RxTx" => Ok((
            Some(Device::new(dev.clone(), sdr_config.clone())),
            Some(Device::new(dev, sdr_config)),
        )),
        _ => Err(anyhow::anyhow!("Invalid direction (Rx/Tx)")),
    }
}
fn open_virtual(config: config::Device) -> anyhow::Result<(Option<Device>, Option<Device>)> {
    let (driver, plugin_path) = INTERNAL_DEVICE_INFO.get("virtual").unwrap();
    let config::Device::Virtual { direction } = config else {
        return Err(anyhow::anyhow!("Invalid config"));
    };

    log::trace!("driver: {}, plugin_path: {}", driver, plugin_path);

    let dev =
        RawDevice::new(format!("driver={}", driver).as_str()).context("failed to open device")?;

    let sdr_config = SDRConfig {
        channels: 0,
        num_channels: NUM_CHANNELS,
        center_freq: 2427e6, // (TODO: add freqency to config)
        freq_mhz: 2427,
        sample_rate: NUM_CHANNELS as f64 * 1.0e6,
        bandwidth: NUM_CHANNELS as f64 * 1.0e6,
        gain: 64.,
    };

    match direction.as_str() {
        "Rx" => Ok((Some(Device::new(dev, sdr_config)), None)),
        "Tx" => Ok((None, Some(Device::new(dev, sdr_config)))),
        "RxTx" => Ok((
            Some(Device::new(dev.clone(), sdr_config.clone())),
            Some(Device::new(dev, sdr_config)),
        )),
        _ => Err(anyhow::anyhow!("Invalid direction (Rx/Tx)")),
    }
}
fn open_file(config: config::Device) -> anyhow::Result<(Option<Device>, Option<Device>)> {
    let (driver, plugin_path) = INTERNAL_DEVICE_INFO.get("file").unwrap();
    let config::Device::File { direction, path } = config else {
        return Err(anyhow::anyhow!("Invalid config"));
    };

    log::trace!("driver: {}, plugin_path: {}", driver, plugin_path);

    let dev = RawDevice::new(format!("driver={},path={}", driver, path).as_str())
        .context("failed to open device")?;

    let sdr_config = SDRConfig {
        channels: 0,
        num_channels: NUM_CHANNELS,
        center_freq: 2427e6, // (TODO: add freqency to config)
        freq_mhz: 2427,
        sample_rate: NUM_CHANNELS as f64 * 1.0e6,
        bandwidth: NUM_CHANNELS as f64 * 1.0e6,
        gain: 64.,
    };

    match direction.as_str() {
        "Rx" => Ok((Some(Device::new(dev, sdr_config)), None)),
        "Tx" => Ok((None, Some(Device::new(dev, sdr_config)))),
        "RxTx" => Ok((
            Some(Device::new(dev.clone(), sdr_config.clone())),
            Some(Device::new(dev, sdr_config)),
        )),
        _ => Err(anyhow::anyhow!("Invalid direction (Rx/Tx)")),
    }
}

fn append_plugin_path() {
    let base = Path::new(env!("OUT_DIR"));

    for (key, (_driver, plugin_path)) in INTERNAL_DEVICE_INFO.iter() {
        log::trace!(
            "appending plugin... (key: {}, plugin_path: {})",
            key,
            plugin_path
        );

        let current = std::env::var("SOAPY_SDR_PLUGIN_PATH").unwrap_or_default();
        std::env::set_var(
            "SOAPY_SDR_PLUGIN_PATH",
            format!("{}:{}", current, base.join(plugin_path).display()),
        );
    }
}

// return (rx stream, tx stream)
pub fn open_device(config: config::List) -> anyhow::Result<(Vec<Device>, Vec<Device>)> {
    append_plugin_path();

    let mut ret = (vec![], vec![]);
    for dev_conf in config.devices {
        let (rx, tx) = match dev_conf {
            config::Device::HackRF { .. } => open_hackrf(dev_conf)?,
            config::Device::Virtual { .. } => open_virtual(dev_conf)?,
            config::Device::File { .. } => open_file(dev_conf)?,
        };

        if let Some(rx) = rx {
            ret.0.push(rx);
        }
        if let Some(tx) = tx {
            ret.1.push(tx);
        }
    }

    Ok(ret)
}

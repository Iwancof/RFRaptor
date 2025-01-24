pub mod sdr;

use std::{path::Path, sync::Mutex};

use anyhow::Context;
use soapysdr::{Device as RawDevice, Direction};

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

fn direction_from_str(s: &str) -> anyhow::Result<Vec<Direction>> {
    match s {
        "Rx" => Ok(vec![Direction::Rx]),
        "Tx" => Ok(vec![Direction::Tx]),
        "RxTx" => Ok(vec![Direction::Rx, Direction::Tx]),
        _ => Err(anyhow::anyhow!("Invalid direction")),
    }
}

const NUM_CHANNELS: usize = 16usize;
// const NUM_CHANNELS: usize = 2usize;

fn open_hackrf(config: config::Device) -> anyhow::Result<Device> {
    let driver = "hackrf";

    let config::Device::HackRF {
        direction,
        freq_mhz,
        serial,
    } = config
    else {
        return Err(anyhow::anyhow!("Invalid config"));
    };

    let directions = direction_from_str(direction.as_str())?;

    log::trace!("driver: {}, serial: {}", driver, serial);

    let dev = RawDevice::new(format!("driver={},serial={}", driver, serial).as_str())
        .context("failed to open device")?;

    let sdr_config = SDRConfig {
        driver: driver.to_string(),
        channels: 0,
        num_channels: NUM_CHANNELS,
        center_freq: freq_mhz as f64 * 1.0e6,
        freq_mhz,
        sample_rate: NUM_CHANNELS as f64 * 1.0e6,
        bandwidth: NUM_CHANNELS as f64 * 1.0e6,
        gain: if directions.contains(&Direction::Tx) {
            32. + 14.
        } else {
            64.
        },
        directions,
        // FIXME: separate rx/tx gain
    };

    sdr_config.set(&dev)?;

    Ok(Device::new(dev, sdr_config))
}
fn open_virtual(config: config::Device) -> anyhow::Result<Device> {
    let driver = "virtual";

    let config::Device::Virtual { direction } = config else {
        return Err(anyhow::anyhow!("Invalid config"));
    };

    let directions = direction_from_str(direction.as_str())?;

    log::trace!("driver: {}", driver);

    let dev =
        RawDevice::new(format!("driver={}", driver).as_str()).context("failed to open device")?;

    let sdr_config = SDRConfig {
        driver: driver.to_string(),
        directions,
        channels: 0,
        num_channels: NUM_CHANNELS,
        center_freq: 2427e6, // (TODO: add freqency to config)
        freq_mhz: 2427,
        sample_rate: NUM_CHANNELS as f64 * 1.0e6,
        bandwidth: NUM_CHANNELS as f64 * 1.0e6,
        gain: 64.,
    };

    sdr_config.set(&dev)?;

    Ok(Device::new(dev, sdr_config))
}
fn open_file(config: config::Device) -> anyhow::Result<Device> {
    let driver = "file";

    let config::Device::File { direction, path } = config else {
        return Err(anyhow::anyhow!("Invalid config"));
    };

    let directions = direction_from_str(direction.as_str())?;

    log::trace!("driver: {}", driver);

    let dev = RawDevice::new(format!("driver={},path={}", driver, path).as_str())
        .context("failed to open device")?;

    let sdr_config = SDRConfig {
        driver: driver.to_string(),
        directions,
        channels: 0,
        num_channels: NUM_CHANNELS,
        center_freq: 2427e6, // (TODO: add freqency to config)
        freq_mhz: 2427,
        sample_rate: NUM_CHANNELS as f64 * 1.0e6,
        bandwidth: NUM_CHANNELS as f64 * 1.0e6,
        gain: 64.,
    };

    sdr_config.set(&dev)?;

    Ok(Device::new(dev, sdr_config))
}

// return (rx stream, tx stream)
pub fn open_device(config: config::List) -> anyhow::Result<Vec<Device>> {
    let base = Path::new(env!("OUT_DIR"));
    let module_path = base.join("lib/SoapySDR/modules0.8");
    log::trace!("module_path: {}", module_path.display());
    std::env::set_var("SOAPY_SDR_PLUGIN_PATH", module_path.display().to_string());

    let mut ret = Vec::new();
    for dev_conf in config.devices {
        let dev = match dev_conf {
            config::Device::HackRF { .. } => open_hackrf(dev_conf)?,
            config::Device::Virtual { .. } => open_virtual(dev_conf)?,
            config::Device::File { .. } => open_file(dev_conf)?,
        };

        ret.push(dev);
    }

    Ok(ret)
}

pub mod sdr;

use std::{
    collections::HashMap,
    path::Path,
    sync::{LazyLock, Mutex},
};

use anyhow::Context;
use soapysdr::Device;

use sdr::SDRConfig;
use serde_yaml;

mod config {
    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    pub(super) enum Device {
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
    pub(super) struct List {
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

const NUM_CHANNELS: usize = 20usize;

pub static SDR_RX_CONFIGS: LazyLock<Mutex<HashMap<usize, SDRConfig>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub static SDR_TX_CONFIGS: LazyLock<Mutex<HashMap<usize, SDRConfig>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn open_hackrf(config: config::Device, ret: &mut (Vec<Device>, Vec<Device>)) -> anyhow::Result<()> {
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

    let dev = soapysdr::Device::new(format!("driver={},serial={}", driver, serial).as_str())
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
        "Rx" => {
            let idx = ret.0.len();
            ret.0.push(dev);
            SDR_RX_CONFIGS.lock().unwrap().insert(idx, sdr_config);
        }
        "Tx" => {
            let idx = ret.1.len();
            ret.1.push(dev);
            SDR_TX_CONFIGS.lock().unwrap().insert(idx, sdr_config);
        }
        "RxTx" => {
            let idx = ret.0.len();
            ret.0.push(dev.clone());
            SDR_RX_CONFIGS
                .lock()
                .unwrap()
                .insert(idx, sdr_config.clone());

            let idx = ret.1.len();
            ret.1.push(dev);
            SDR_TX_CONFIGS.lock().unwrap().insert(idx, sdr_config);
        }
        _ => return Err(anyhow::anyhow!("Invalid direction (Rx/Tx)")),
    };

    Ok(())
}
fn open_virtual(
    config: config::Device,
    ret: &mut (Vec<Device>, Vec<Device>),
) -> anyhow::Result<()> {
    let (driver, plugin_path) = INTERNAL_DEVICE_INFO.get("virtual").unwrap();
    let config::Device::Virtual { direction } = config else {
        return Err(anyhow::anyhow!("Invalid config"));
    };

    log::trace!("driver: {}, plugin_path: {}", driver, plugin_path);

    let dev = soapysdr::Device::new(format!("driver={}", driver).as_str())
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
        "Rx" => {
            let idx = ret.0.len();
            ret.0.push(dev);
            SDR_RX_CONFIGS.lock().unwrap().insert(idx, sdr_config);
        }
        "Tx" => {
            let idx = ret.1.len();
            ret.1.push(dev);
            SDR_TX_CONFIGS.lock().unwrap().insert(idx, sdr_config);
        }
        "RxTx" => {
            let idx = ret.0.len();
            ret.0.push(dev.clone());
            SDR_RX_CONFIGS
                .lock()
                .unwrap()
                .insert(idx, sdr_config.clone());

            let idx = ret.1.len();
            ret.1.push(dev);
            SDR_TX_CONFIGS.lock().unwrap().insert(idx, sdr_config);
        }
        _ => return Err(anyhow::anyhow!("Invalid direction (Rx/Tx)")),
    };

    Ok(())
}
fn open_file(config: config::Device, ret: &mut (Vec<Device>, Vec<Device>)) -> anyhow::Result<()> {
    let (driver, plugin_path) = INTERNAL_DEVICE_INFO.get("file").unwrap();
    let config::Device::File { direction, path } = config else {
        return Err(anyhow::anyhow!("Invalid config"));
    };

    log::trace!("driver: {}, plugin_path: {}", driver, plugin_path);

    let dev = soapysdr::Device::new(format!("driver={},path={}", driver, path).as_str())
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
        "Rx" => {
            let idx = ret.0.len();
            ret.0.push(dev);
            SDR_RX_CONFIGS.lock().unwrap().insert(idx, sdr_config);
        }
        _ => return Err(anyhow::anyhow!("Invalid direction (Rx)")),
    };

    Ok(())
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
pub fn open_device(config_path: String) -> anyhow::Result<(Vec<Device>, Vec<Device>)> {
    append_plugin_path();

    let file = std::fs::File::open(config_path)?;

    let config: config::List = serde_yaml::from_reader(file).context("failed to parse config")?;
    // println!("{:?}", config);

    let mut ret = (vec![], vec![]);
    for dev_conf in config.devices {
        match dev_conf {
            config::Device::HackRF { .. } => {
                open_hackrf(dev_conf, &mut ret)?;
            }
            config::Device::Virtual { .. } => {
                open_virtual(dev_conf, &mut ret)?;
            }
            config::Device::File { .. } => {
                open_file(dev_conf, &mut ret)?;
            }
        }
    }

    Ok(ret)
}

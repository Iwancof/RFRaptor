use core::fmt;

use anyhow::Context;
use soapysdr::Direction::{Rx, Tx};

#[log_derive::logfn(ok = "TRACE", err = "ERROR")]
fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    soapysdr::configure_logging();

    let filter = "hackrf";
    log::trace!("filter is {}", filter);

    let devarg = soapysdr::enumerate(filter)
        .context("failed to enumerate devices")?
        .into_iter()
        .next()
        .context("No devices found")?;
    log::trace!("found device {}", devarg);

    let dev = soapysdr::Device::new(devarg)?;

    let config = SDRConfig {
        channels: 20,
        center_freq: 2441.0e6, // bluetooth
        sample_rate: 20.0e6,
        bandwidth: 20.0e6,
        gain: 20.0,
    };

    log::info!("config = {}", config);
    config.set(&dev)?;

    Ok(())
}

struct SDRConfig {
    channels: usize,
    center_freq: f64,
    sample_rate: f64,
    bandwidth: f64,
    gain: f64,
}

impl SDRConfig {
    fn set(&self, dev: &soapysdr::Device) -> anyhow::Result<()> {
        for channel in 0..self.channels {
            dev.set_frequency(Rx, channel, self.center_freq, ())?;
            dev.set_sample_rate(Rx, channel, self.sample_rate)?;
            dev.set_bandwidth(Rx, channel, self.bandwidth)?;
            dev.set_gain(Rx, channel, self.gain)?;
        }
        Ok(())
    }
}

impl fmt::Display for SDRConfig {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "channels: {}, center_freq: {}, sample_rate: {}, bandwidth: {}, gain: {}",
            self.channels, self.center_freq, self.sample_rate, self.bandwidth, self.gain
        )
    }
}

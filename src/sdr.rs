use soapysdr::Direction::Rx;

#[derive(Debug, Clone)]
pub struct SDRConfig {
    pub channels: usize,
    pub center_freq: f64,
    pub sample_rate: f64,
    pub bandwidth: f64,
    pub gain: f64,
}

impl SDRConfig {
    pub fn set(&self, dev: &soapysdr::Device) -> anyhow::Result<()> {
        for channel in 0..self.channels {
            dev.set_frequency(Rx, channel, self.center_freq, ())?;
            dev.set_sample_rate(Rx, channel, self.sample_rate)?;
            dev.set_bandwidth(Rx, channel, self.bandwidth)?;
            dev.set_gain(Rx, channel, self.gain)?;
        }
        Ok(())
    }
}

impl core::fmt::Display for SDRConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "channels: {}, center_freq: {}, sample_rate: {}, bandwidth: {}, gain: {}",
            self.channels, self.center_freq, self.sample_rate, self.bandwidth, self.gain
        )
    }
}

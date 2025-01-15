#[derive(Debug, Clone)]
pub struct SDRConfig {
    pub driver: String,

    pub directions: Vec<soapysdr::Direction>,

    /// SDR antenna channel
    pub channels: usize,

    /// Number of channels to view
    pub num_channels: usize,

    /// Center frequency of the SDR [Hz]
    pub center_freq: f64,

    #[doc(hidden)]
    pub freq_mhz: usize,

    /// Sample rate of the SDR
    pub sample_rate: f64,

    /// Bandwidth of the SDR
    pub bandwidth: f64,

    /// Gain of the SDR
    pub gain: f64,
}

impl SDRConfig {
    pub fn set(&self, dev: &soapysdr::Device) -> anyhow::Result<()> {
        // for channel in 0..=self.channels {
        //     dev.set_frequency(Rx, channel, self.center_freq, ())?;
        //     dev.set_sample_rate(Rx, channel, self.sample_rate)?;
        //     dev.set_bandwidth(Rx, channel, self.bandwidth)?;
        //     dev.set_gain(Rx, channel, self.gain)?;
        // }

        for direction in &self.directions {
            for channel in 0..self.num_channels {
                dev.set_frequency(*direction, channel, self.center_freq, ())?;
                dev.set_sample_rate(*direction, channel, self.sample_rate)?;
                dev.set_bandwidth(*direction, channel, self.bandwidth)?;
                dev.set_gain(*direction, channel, self.gain)?;
            }
        }

        Ok(())
    }
}

impl core::fmt::Display for SDRConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "SDRConfig {{ driver: {}, directions: {:?}, channels: {}, num_channels: {}, center_freq: {}, sample_rate: {}, bandwidth: {}, gain: {} }}",
            self.driver, self.directions, self.channels, self.num_channels, self.center_freq, self.sample_rate, self.bandwidth, self.gain
        )
    }
}

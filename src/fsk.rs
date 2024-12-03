use liquid_dsp_sys::{
    fskdem, fskdem_create, fskdem_demodulate, fskdem_destroy, fskdem_reset, fskmod, fskmod_create,
    fskmod_destroy, fskmod_modulate, fskmod_reset,
};

const DEFAULT_FSK_BANDWIDTH: f32 = 0.40; // ????

fn do_liquid<Ret, F: FnOnce() -> *mut Ret>(f: F) -> anyhow::Result<*mut Ret> {
    use wrcap::lent_stderr;

    let (ret, error) = lent_stderr()
        .map_err(|_| anyhow::anyhow!("failed to lent stderr"))?
        .capture_string(f)?;

    if !ret.is_null() {
        return Ok(ret);
    }

    use regex::Regex;
    let re = Regex::new(r"error \[([0-9]+)\]: (.*)\n  (.*)")?;
    let Some(capture) = re.captures(&error) else {
        return Err(anyhow::anyhow!("error: {:?}", error));
    };

    let content = capture.get(2).ok_or(anyhow::anyhow!("parse content"))?.as_str();
    let source = capture
        .get(3)
        .ok_or(anyhow::anyhow!("parse line"))?
        .as_str();

    anyhow::bail!("[{}] at [{}]", content, source);
}

/// FSK demodulator
#[derive(Debug)]
pub struct FskDemod {
    #[doc(hidden)]
    fskdem: fskdem,

    /// The number of samples per symbol
    #[allow(unused)]
    sample_per_symbol: u32,

    /// The number of bits per symbol
    #[allow(unused)]
    bits_per_symbol: u32,
}

impl Drop for FskDemod {
    fn drop(&mut self) {
        unsafe {
            fskdem_destroy(self.fskdem); // not fail.
        }
    }
}

fn prepare_fftw3f_thread_safety() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        fftwf_make_planner_thread_safe();
    });

    #[link(name = "fftw3f_threads")]
    extern "C" {
        fn fftwf_make_planner_thread_safe();
    }
}

impl FskDemod {
    const DEFAULT_DEMOULATE_BANDWITH: f32 = DEFAULT_FSK_BANDWIDTH;

    /// Create a new FSK demodulator
    ///
    /// # Arguments
    /// * `sample_rate` [Hz] - The sample rate of the incoming data
    /// * `num_channels` - The number of channels to use
    /// * `bandwidth` - The bandwidth of the demodulator
    pub fn new_with_band(sample_rate: f32, num_channels: usize, bandwidth: f32) -> Self {
        prepare_fftw3f_thread_safety();

        let sample_per_symbol = (sample_rate / (num_channels as f32) / 1e6f32 * 2.0) as u32;
        let bits_per_symbol = sample_per_symbol.trailing_zeros();

        let fskdem =
            do_liquid(|| unsafe { fskdem_create(bits_per_symbol, sample_per_symbol, bandwidth) })
                .expect("fskdem_create failed");

        Self {
            fskdem,
            sample_per_symbol,
            bits_per_symbol,
        }
    }

    /// Create a new FSK demodulator
    ///
    /// # Arguments
    /// * `sample_rate` [Hz] - The sample rate of the incoming data
    /// * `num_channels` - The number of channels to use
    pub fn new(sample_rate: f32, num_channels: usize) -> Self {
        Self::new_with_band(sample_rate, num_channels, Self::DEFAULT_DEMOULATE_BANDWITH)
    }

    /// Demodulate the data
    pub fn demodulate(&mut self, data: &[num_complex::Complex<f32>]) -> Option<Vec<u8>> {
        unsafe {
            fskdem_reset(self.fskdem);
        }

        let mut bits = Vec::new();
        for d in data.chunks(self.sample_per_symbol as usize) {
            // TODO: only support 2 samples per symbol
            let bit = unsafe {
                // TODO: check return value
                fskdem_demodulate(self.fskdem, d.as_ptr() as *mut _)
            };

            bits.push(bit as u8);
        }

        Some(bits)
    }
}

#[derive(Debug)]
pub struct FskMod {
    #[doc(hidden)]
    fskmod: fskmod,

    /// The number of samples per symbol
    #[allow(unused)]
    sample_per_symbol: u32,

    /// The number of bits per symbol
    #[allow(unused)]
    bits_per_symbol: u32,
}

impl Drop for FskMod {
    fn drop(&mut self) {
        unsafe {
            fskmod_destroy(self.fskmod);
        }
    }
}

impl FskMod {
    const DEFAULT_MODULATE_BANDWITH: f32 = DEFAULT_FSK_BANDWIDTH;

    /// Create a new FSK modulator
    ///
    /// # Arguments
    /// * `sample_rate` [Hz] - The sample rate of the transmitted data
    /// * `num_channels` - The number of channels to use
    pub fn new_with_band(sample_rate: f32, num_channels: usize, bandwidth: f32) -> Self {
        prepare_fftw3f_thread_safety();

        let sample_per_symbol = (sample_rate / (num_channels as f32) / 1e6f32 * 2.0) as u32;
        let bits_per_symbol = sample_per_symbol.trailing_zeros();

        let fskmod =
            do_liquid(|| unsafe { fskmod_create(bits_per_symbol, sample_per_symbol, bandwidth) })
                .expect("fskmod_create failed");

        Self {
            fskmod,
            sample_per_symbol,
            bits_per_symbol,
        }
    }

    /// Create a new FSK modulator
    ///
    /// # Arguments
    /// * `sample_rate` [Hz] - The sample rate of the transmitted data
    /// * `num_channels` - The number of channels to use
    pub fn new(sample_rate: f32, num_channels: usize) -> Self {
        Self::new_with_band(sample_rate, num_channels, Self::DEFAULT_MODULATE_BANDWITH)
    }

    pub fn modulate(&mut self, data: &[u8]) -> Vec<num_complex::Complex<f32>> {
        let mut modulated = Vec::new();

        unsafe {
            fskmod_reset(self.fskmod);
        }

        for d in data {
            let mut out =
                vec![num_complex::Complex::new(0.0, 0.0); self.sample_per_symbol as usize];
            // TODO: only support 2 samples per symbol
            unsafe {
                // TODO: check return value
                fskmod_modulate(self.fskmod, *d as u32, out.as_mut_ptr());
            }

            modulated.extend_from_slice(&out);
        }

        modulated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    include!("./def_test_data/fsk.rs");

    #[test]
    fn test_simple_demod() {
        let mut fsk = FskDemod::new(20e6, 20);
        let packet = fsk.demodulate(&EXPECT_DATA_1_FREQ).expect("demod failed");

        // assert_eq!(packet.bits, EXPECT_DATA_1_BITS);
        let mut min = useful_number::updatable_num::UpdateToMinU32::new();

        for offset in 0..3 {
            let mut xor_count = 0;
            packet[offset..]
                .iter()
                .zip(EXPECT_DATA_1_BITS.iter())
                .for_each(|(a, b)| {
                    if a != b {
                        xor_count += 1;
                    }
                });

            min.update(xor_count);
        }
        for offset in 0..3 {
            let mut xor_count = 0;
            EXPECT_DATA_1_BITS[offset..]
                .iter()
                .zip(packet.iter())
                .for_each(|(a, b)| {
                    if a != b {
                        xor_count += 1;
                    }
                });

            min.update(xor_count);
        }

        // assert!(min < 10);

        let min = *min.get().expect("min failed");
        let error_rate = min as f32 / EXPECT_DATA_1_BITS.len() as f32;
        assert!(error_rate < 0.05);
    }

    #[test]
    fn test_simple_modul() {
        let mut modulater = FskMod::new(20e6, 20);
        let packet = EXPECT_DATA_1_BITS.to_vec();

        let modulated = modulater.modulate(&packet);

        let mut demodulater = FskDemod::new(20e6, 20);
        let demodulated = demodulater.demodulate(&modulated).expect("demod failed");

        assert_eq!(packet, demodulated);
    }

    #[should_panic]
    #[test]
    fn do_liquid_test() {
        let mut invalid_config = FskDemod::new_with_band(20e6, 20, 0.50);
    }
}

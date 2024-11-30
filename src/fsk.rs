use liquid_dsp_sys::{fskdem, fskdem_create, fskdem_destroy, fskdem_reset, fskdem_demodulate};

/// FSK demodulator
#[derive(Debug)]
pub struct FskDemod {
    fskdem: fskdem,
}

impl Drop for FskDemod {
    fn drop(&mut self) {
        unsafe {
            fskdem_destroy(self.fskdem);
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
    /// Create a new FSK demodulator
    ///
    /// # Arguments
    /// * `sample_rate` [Hz] - The sample rate of the incoming data
    /// * `num_channels` - The number of channels to use
    pub fn new(sample_rate: f32, num_channels: usize) -> Self {
        prepare_fftw3f_thread_safety();

        let sample_per_symbol = (sample_rate / (num_channels as f32) / 1e6f32 * 2.0) as u32;
        assert_eq!(sample_per_symbol, 2); // FIXME: only support 2 samples per symbol.
                                          // m = 1, 2 ** m = sample_per_symbol = 2
        let fskdem = unsafe { fskdem_create(1, sample_per_symbol, 0.4) };

        Self { fskdem }
    }

    /// Demodulate the data
    pub fn demod(&mut self, data: &[num_complex::Complex<f32>]) -> Option<Vec<u8>> {
        unsafe {
            fskdem_reset(self.fskdem);
        }

        let mut bits = Vec::new();
        for d in data.chunks(2) {
            let bit = unsafe {
                fskdem_demodulate(self.fskdem, d.as_ptr() as *const _ as *mut _)
            };

            bits.push(bit as u8);
        }

        Some(bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    include!("./fsk_define_test.rs");

    #[test]
    fn test_simple_demod() {
        let mut fsk = FskDemod::new(20e6, 20);
        let packet = fsk.demod(&EXPECT_DATA_1_FREQ).expect("demod failed");

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

        let min = *min.get().unwrap();
        let error_rate = min as f32 / EXPECT_DATA_1_BITS.len() as f32;
        assert!(error_rate < 0.05);
    }
}

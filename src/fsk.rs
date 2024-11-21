use liquid_dsp_sys::{freqdem, freqdem_create, freqdem_destroy};

use num_complex::Complex;
use num_traits::Signed;

/// at least 64 symbols are needed to calculate the median
const MEDIAN_SYMBOLS: usize = 64usize;

/// FSK demodulator
#[derive(Debug)]
pub struct FskDemod {
    #[allow(unused)]
    freqdem: freqdem,

    /// number of samples per symbol
    #[allow(unused)]
    pub sample_per_symbol: usize,

    /// number of symbols needed to calculate the median
    #[allow(unused)]
    pub need_symbol: usize,

    /// limit of the frequency offset
    #[allow(unused)]
    pub max_freq_offset: f32,
}

/// FSK demodulated packet
#[derive(Debug)]
pub struct Packet {
    /// demodulated bits
    #[allow(unused)]
    pub bits: Vec<u8>,

    /// demodulated data
    #[allow(unused)]
    pub demod: Vec<f32>,

    /// CFO (Carrier Frequency Offset)
    #[allow(unused)]
    pub cfo: f32,

    /// frequency deviation
    #[allow(unused)]
    pub deviation: f32,
}

impl Drop for FskDemod {
    fn drop(&mut self) {
        unsafe {
            freqdem_destroy(self.freqdem);
        }
    }
}

impl FskDemod {
    /// Create a new FSK demodulator
    ///
    /// # Arguments
    /// * `sample_rate` [Hz] - The sample rate of the incoming data
    /// * `num_channels` - The number of channels to use
    pub fn new(sample_rate: f32, num_channels: usize) -> Self {
        let freqdem = unsafe { freqdem_create(0.8f32) };

        let sample_per_symbol = (sample_rate / (num_channels as f32) / 1e6f32 * 2.0) as usize;
        Self {
            freqdem,
            sample_per_symbol,
            need_symbol: MEDIAN_SYMBOLS,
            max_freq_offset: 0.4f32,
        }
    }

    // Number of samples needed to calculate the median
    fn median_size(&self) -> usize {
        self.sample_per_symbol * self.need_symbol
    }

    // Raw demodulation
    fn liquid_demod(&mut self, data: &[Complex<f32>]) -> Vec<f32> {
        use liquid_dsp_sys::*;

        let mut demod: Vec<f32> = Vec::with_capacity(data.len());

        unsafe {
            freqdem_reset(self.freqdem);

            freqdem_demodulate_block(
                self.freqdem,
                data.as_ptr() as *mut __BindgenComplex<f32>,
                data.len() as _,
                demod.as_mut_ptr(),
            );

            demod.set_len(data.len());
        }

        demod
    }

    /// Demodulate the data
    pub fn demod(&mut self, data: &[Complex<f32>]) -> Option<Packet> {
        // too short to demodulate
        if data.len() < 8 + self.median_size() {
            return None;
        }

        // demodulate the data
        let mut demod = self.liquid_demod(data);

        // get the CFO and deviation
        let (cfo, deviation) = self.correction(&demod)?;
        demod.iter_mut().for_each(|d| {
            *d -= cfo;
            *d /= deviation;
        });

        // prepare to calculate the EWMA
        if demod[0].abs() > 1.5 {
            demod[0] = 0.;
        }

        let mut ewma = 0.;
        let bits = demod
            .iter()
            // skip silence at the beginning
            .skip_while(|v| {
                const ALPHA: f32 = 0.8;
                ewma = ewma * (1. - ALPHA) + v.abs() * ALPHA;

                ewma <= 0.5
            })
            // each symbol has 2 samples (?)
            .step_by(2)
            .map(|v| if v > &0.0 { 1 } else { 0 })
            .collect::<Vec<u8>>();

        Some(Packet {
            bits,
            demod,
            cfo,
            deviation,
        })
    }

    // Calculate the CFO and deviation
    fn correction(&self, demod: &[f32]) -> Option<(f32, f32)> {
        let mut pos = Vec::new();
        let mut neg = Vec::new();

        for d in demod.iter().skip(8).take(self.median_size()) {
            // too large frequency offset
            if d.abs() > self.max_freq_offset {
                return None;
            }

            if d.is_positive() {
                pos.push(*d);
            } else {
                neg.push(*d);
            }
        }

        // the data is too skewed
        if pos.len() < self.need_symbol / 4 || neg.len() < self.need_symbol / 4 {
            return None;
        }

        // sort the data
        pos.sort_by(|a, b| a.partial_cmp(b).unwrap());
        neg.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // calculate the median excluding the outliers
        let median = (pos[pos.len() * 3 / 4] + neg[neg.len() / 4]) / 2.0;

        let cfo = median;
        let deviation = pos[pos.len() * 3 / 4] - median;

        Some((cfo, deviation))
    }
}

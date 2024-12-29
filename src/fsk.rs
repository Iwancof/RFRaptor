use std::ptr::NonNull;

use crate::{
    burst,
    liquid::{liquid_do_int, liquid_get_pointer},
};

use anyhow::Context;
use num_complex::Complex;

use liquid_dsp_sys::{
    freqdem, freqdem_create, freqdem_destroy, freqdem_s, freqmod, freqmod_create, freqmod_destroy,
    freqmod_modulate_block, freqmod_reset, freqmod_s,
};
use num_traits::Signed;

/// at least 64 symbols are needed to calculate the median
const MEDIAN_SYMBOLS: usize = 64usize;

/// FSK demodulator
#[derive(Debug)]
pub struct FskDemod {
    #[allow(unused)]
    freqdem: NonNull<freqdem_s>,

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
#[derive(Debug, Clone)]
pub struct Packet {
    #[allow(unused)]
    pub raw: Option<burst::Packet>,

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
            liquid_do_int(|| freqdem_destroy(self.freqdem())).expect("freqdem_destroy failed");
        }
    }
}

impl FskDemod {
    fn freqdem(&self) -> freqdem {
        self.freqdem.as_ptr()
    }

    /// Create a new FSK demodulator
    ///
    /// # Arguments
    /// * `sample_rate` [Hz] - The sample rate of the incoming data
    /// * `num_channels` - The number of channels to use
    pub fn new(sample_rate: f32, num_channels: usize) -> Self {
        let freqdem = liquid_get_pointer(|| unsafe { freqdem_create(0.8f32) })
            .expect("freqdem_create failed");
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
    fn liquid_demod(&mut self, data: &[Complex<f32>]) -> anyhow::Result<Vec<f32>> {
        use liquid_dsp_sys::*;

        let mut demod: Vec<f32> = Vec::with_capacity(data.len());

        unsafe {
            liquid_do_int(|| freqdem_reset(self.freqdem())).context("freqdem_reset failed")?;

            // TODO: add safety checks
            liquid_do_int(|| {
                freqdem_demodulate_block(
                    self.freqdem(),
                    data.as_ptr() as *mut _,
                    data.len() as _,
                    demod.as_mut_ptr(),
                )
            })
            .context("freqdem_demodulate_block failed")?;

            demod.set_len(data.len());
        }

        Ok(demod)
    }

    pub fn demodulate(&mut self, packet: burst::Packet) -> anyhow::Result<Packet> {
        let demodulated = self.demodulate_signal(&packet.data)?;

        Ok(Packet {
            raw: Some(packet),
            ..demodulated
        })
    }

    /// Demodulate the data
    pub fn demodulate_signal(&mut self, data: &[Complex<f32>]) -> anyhow::Result<Packet> {
        // too short to demodulate
        if data.len() < 8 + self.median_size() {
            anyhow::bail!("data is too short");
        }

        // demodulate the data
        let mut demod = self.liquid_demod(data)?;

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
            .step_by(self.sample_per_symbol)
            .map(|v| if v > &0.0 { 1 } else { 0 })
            .collect::<Vec<u8>>();

        Ok(Packet {
            raw: None,
            bits,
            demod,
            cfo,
            deviation,
        })
    }

    // Calculate the CFO and deviation
    fn correction(&self, demod: &[f32]) -> anyhow::Result<(f32, f32)> {
        let mut pos = Vec::new();
        let mut neg = Vec::new();

        for d in demod.iter().skip(8).take(self.median_size()) {
            // too large frequency offset
            if d.abs() > self.max_freq_offset {
                anyhow::bail!("frequency offset is too large");
            }

            if d.is_positive() {
                pos.push(*d);
            } else {
                neg.push(*d);
            }
        }

        // the data is too skewed
        if pos.len() < self.need_symbol / 4 || neg.len() < self.need_symbol / 4 {
            anyhow::bail!("data is too skewed");
        }

        // sort the data
        pos.sort_by(|a, b| a.partial_cmp(b).unwrap());
        neg.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // calculate the median excluding the outliers
        let median = (pos[pos.len() * 3 / 4] + neg[neg.len() / 4]) / 2.0;

        let cfo = median;
        let deviation = pos[pos.len() * 3 / 4] - median;

        Ok((cfo, deviation))
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct FskMod {
    #[doc(hidden)]
    freqmod: NonNull<freqmod_s>,

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
            liquid_do_int(|| freqmod_destroy(self.freqmod())).expect("freqmod_destroy failed");
        }
    }
}

#[allow(dead_code)]
impl FskMod {
    const DEFAULT_MODULATE_BANDWITH: f32 = 0.4;

    /// Create a new FSK modulator
    ///
    /// # Arguments
    /// * `sample_rate` [Hz] - The sample rate of the transmitted data
    /// * `num_channels` - The number of channels to use
    pub fn new(sample_rate: f32, num_channels: u32) -> Self {
        let freqmod =
            liquid_get_pointer(|| unsafe { freqmod_create(0.8f32) }).expect("fskmod_create failed");

        let sample_per_symbol = (sample_rate / (num_channels as f32) / 1e6f32 * 2.0) as u32;
        let bits_per_symbol = sample_per_symbol.trailing_zeros();

        Self {
            freqmod,
            sample_per_symbol,
            bits_per_symbol,
        }
    }

    fn freqmod(&self) -> freqmod {
        self.freqmod.as_ptr()
    }

    fn liquid_modulate(&mut self, data: &[f32]) -> anyhow::Result<Vec<num_complex::Complex<f32>>> {
        let mut modulated = Vec::with_capacity(data.len());

        unsafe {
            liquid_do_int(|| freqmod_reset(self.freqmod())).context("freqmod_reset failed")?;

            // TODO: add safety checks
            liquid_do_int(|| {
                freqmod_modulate_block(
                    self.freqmod(),
                    data.as_ptr() as *mut _,
                    data.len() as _,
                    modulated.as_mut_ptr(),
                )
            })
            .context("fskmod_modulate failed")?;

            modulated.set_len(data.len());
        }

        Ok(modulated)
    }

    pub fn modulate(&mut self, data: &[u8]) -> anyhow::Result<Vec<num_complex::Complex<f32>>> {
        let mut f = vec![];

        f.extend(data.iter().flat_map(|b| {
            (0..=self.bits_per_symbol).map(move |_| if b & 1 != 0 { -1.0 } else { 1.0 })
        }));

        self.liquid_modulate(&f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    include!("./def_test_data/fsk.rs");

    #[test]
    fn test_simple_demod() {
        let mut fsk = FskDemod::new(20e6, 20);
        let packet = fsk
            .demodulate_signal(&EXPECT_DATA_1_FREQ)
            .expect("demod failed");

        // assert_eq!(packet.bits, EXPECT_DATA_1_BITS);
        let mut min = useful_number::updatable_num::UpdateToMinU32::new();

        for offset in 0..3 {
            let mut xor_count = 0;
            packet.bits[offset..]
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
                .zip(packet.bits.iter())
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

        let modulated = modulater.modulate(&packet).expect("modul failed");
        println!("{:?}", modulated);

        let mut demodulater = FskDemod::new(20e6, 20);
        let demodulated = demodulater
            .demodulate_signal(&modulated)
            .expect("demod failed");

        assert_eq!(packet, demodulated.bits);
    }

    #[test]
    fn compatible_data_test() {
        use std::io::BufRead;

        let file = std::fs::File::open("./fsk_debug.txt").expect("file open failed");
        let reader = std::io::BufReader::new(file);
        let mut lines = reader.lines();

        let mut data: Vec<(Vec<num_complex::Complex32>, Option<Vec<u8>>)> = Vec::new();

        while let Some(line) = lines.next() {
            let raw_len: usize = line.unwrap().trim().parse().unwrap();

            let mut raw_data = Vec::new();
            for _ in 0..raw_len {
                let line = lines.next().unwrap().unwrap();
                let mut parts = line.split_whitespace();

                let real: f32 = parts.next().unwrap().parse().unwrap();
                let imag: f32 = parts.next().unwrap().parse().unwrap();

                raw_data.push(num_complex::Complex32::new(real, imag));
            }

            let bits_len: usize = lines.next().unwrap().unwrap().trim().parse().unwrap();
            let bits = if bits_len == 0 {
                None
            } else {
                let mut bits = Vec::new();
                for _ in 0..bits_len {
                    let line = lines.next().unwrap().unwrap();
                    let bit: u8 = line.trim().parse().unwrap();
                    bits.push(bit);
                }

                Some(bits)
            };

            data.push((raw_data, bits));
        }

        println!("loaded {:?}", data.len());

        let mut fsk = FskDemod::new(20e6, 20);

        for d in data.iter() {
            let demod = fsk.demodulate_signal(&d.0);

            match demod {
                Ok(demod) => {
                    let expect_bits = d.1.as_ref().unwrap();

                    for (a, b) in demod.bits.iter().zip(expect_bits.iter()) {
                        assert_eq!(a, b);
                    }

                    // if let Ok(p) = crate::bitops::bits_to_packet(&bits, 2427) {
                    //     println!("{:x?}", p);
                    // }
                }
                Err(e) => {
                    if d.1.is_some() {
                        panic!("{:?}", e);
                    }
                }
            }
        }
    }
}

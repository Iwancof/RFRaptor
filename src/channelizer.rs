use core::fmt::Debug;
use std::default;

use az::WrappingAs;

use liquid_dsp_sys::{firpfbch2_crcf_create, firpfbch2_crcf_create_kaiser, LIQUID_ANALYZER, LIQUID_SYNTHESIZER};
use num_complex::Complex;
use num_traits::Float;

use crate::liquid::{liquid_do_int, liquid_get_pointer};

const SYMBOL_DELAY: u32 = 4;

pub struct Channelizer {
    num_channels: usize,

    analyzer: core::ptr::NonNull<liquid_dsp_sys::firpfbch2_crcf_s>,

    #[doc(hidden)]
    channel_half: usize,

    #[doc(hidden)]
    working_buffer: Vec<Complex<f32>>,
    // len(working_buffer) = num_channels
}

pub struct Synthesizer {
    num_channels: usize,

    synthesizer: core::ptr::NonNull<liquid_dsp_sys::firpfbch2_crcf_s>,

    #[doc(hidden)]
    channel_half: usize,

    #[doc(hidden)]
    working_buffer: Vec<Complex<f32>>,
    // len(working_buffer) = num_channels
}

impl Channelizer {
    pub fn new(num_channels: usize) -> Self {
        let analyzer = liquid_get_pointer(|| unsafe {
            // firpfbch2_crcf_create(
            firpfbch2_crcf_create_kaiser(
                LIQUID_ANALYZER as i32,
                num_channels as u32,
                SYMBOL_DELAY,
                // generate_kaiser(num_channels, SYMBOL_DELAY, 0.75).as_mut_ptr(),
                60.0,
            )
        })
        .expect("firpfbch2_crcf_create_kaiser failed (channelizer)");

        Self {
            num_channels,
            channel_half: num_channels / 2,
            analyzer,
            working_buffer: vec![Complex::new(0.0, 0.0); num_channels],
        }
    }

    pub fn channelize(&mut self, input: &[Complex<f32>]) -> &[Complex<f32>] {
        debug_assert_eq!(input.len(), self.channel_half);
        debug_assert_eq!(self.working_buffer.len(), self.num_channels);

        liquid_do_int(|| unsafe {
            liquid_dsp_sys::firpfbch2_crcf_execute(
                self.analyzer.as_ptr(),
                input.as_ptr() as *mut _,
                self.working_buffer.as_mut_ptr(),
            )
        })
        .expect("firpfbch2_crcf_execute failed");

        for d in self.working_buffer.iter_mut() {
            d.re /= 1.5;
            d.im /= 1.5;
        }

        &self.working_buffer
    }
}

impl Synthesizer {
    pub fn new(num_channels: usize) -> Self {
        let synthesizer = liquid_get_pointer(|| unsafe {
            firpfbch2_crcf_create(
                LIQUID_SYNTHESIZER as i32,
                num_channels as u32,
                SYMBOL_DELAY,
                generate_kaiser(num_channels, SYMBOL_DELAY, 0.75 / 2.).as_mut_ptr(),
            )
        })
        .expect("firpfbch2_crcf_create_kaiser failed (synthesizer)");

        Self {
            num_channels,
            channel_half: num_channels / 2,
            synthesizer,
            working_buffer: vec![Complex::new(0.0, 0.0); num_channels / 2],
        }
    }

    pub fn synthesizer(&mut self, input: &[Complex<f32>]) -> &[Complex<f32>] {
        debug_assert_eq!(input.len(), self.num_channels);
        debug_assert_eq!(self.working_buffer.len(), self.channel_half);

        liquid_do_int(|| unsafe {
            liquid_dsp_sys::firpfbch2_crcf_execute(
                self.synthesizer.as_ptr(),
                input.as_ptr() as *mut _,
                self.working_buffer.as_mut_ptr(),
            )
        })
        .expect("firpfbch2_crcf_execute failed");

        &self.working_buffer
    }
}

impl Drop for Channelizer {
    fn drop(&mut self) {
        liquid_do_int(|| unsafe { liquid_dsp_sys::firpfbch2_crcf_destroy(self.analyzer.as_ptr()) })
            .expect("firpfbch2_crcf_destroy failed (channelizer)");
    }
}

impl core::fmt::Display for Channelizer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use wrcap::lent_stdout;

        let ((), content) = lent_stdout().unwrap().capture_string(|| {
            unsafe {
                liquid_dsp_sys::firpfbch2_crcf_print(self.analyzer.as_ptr());
            };
        }).unwrap();

        writeln!(f, "Channelizer")?;
        writeln!(f, "- num_channels: {}", self.num_channels)?;
        writeln!(f, "- analyser: {:p}", self.analyzer)?;

        writeln!(f, "- firpfbch2_crcf_print")?;
        write!(f, "  - {}", content.strip_suffix("\n").unwrap())?;

        Ok(())
    }
}

impl Drop for Synthesizer {
    fn drop(&mut self) {
        liquid_do_int(|| unsafe {
            liquid_dsp_sys::firpfbch2_crcf_destroy(self.synthesizer.as_ptr())
        })
        .expect("firpfbch2_crcf_destroy failed (synthesizer)");
    }
}

impl core::fmt::Display for Synthesizer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use wrcap::lent_stdout;

        let ((), content) = lent_stdout().unwrap().capture_string(|| {
            unsafe {
                liquid_dsp_sys::firpfbch2_crcf_print(self.synthesizer.as_ptr());
            };
        }).unwrap();

        writeln!(f, "Synthesizer")?;
        writeln!(f, "- num_channels: {}", self.num_channels)?;
        writeln!(f, "- synthesizer: {:p}", self.synthesizer)?;

        writeln!(f, "- firpfbch2_crcf_print")?;
        write!(f, "  - {}", content.strip_suffix("\n").unwrap())?;

        Ok(())
    }
}



fn generate_kaiser(channel: usize, m: u32, lp_cutoff: f32) -> Vec<f32> {
    let h_len = 2 * channel * m as usize + 1;
    let mut buffer = Vec::with_capacity(h_len);

    unsafe {
        liquid_dsp_sys::liquid_firdes_kaiser(
            h_len as _,
            lp_cutoff / channel as f32,
            60.0,
            0.0,
            buffer.as_mut_ptr(),
        );

        buffer.set_len(h_len);
    };

    let mut sum = 0.0;
    for x in buffer.iter() {
        sum += x;
    }

    for x in buffer.iter_mut() {
        *x *= channel as f32 / sum;
    }

    buffer
}

#[cfg(test)]
mod test {
    use super::*;
    use approx::relative_eq;
    use num_traits::WrappingAdd;
    use rand::{rngs::SmallRng, Rng, SeedableRng};

    use std::simd::*;

    /*
    #[test]
    fn float_channelizer() {
        let num_channels = 16;
        let mut channelizer = Channelizer::<f32>::new(num_channels, 4, 0.75);

        let div = 127.0;
        let data = [
            Complex::new(0.0 / div, 1.0 / div),
            Complex::new(1.0 / div, 0.0 / div),
            Complex::new(2.0 / div, 1.0 / div),
            Complex::new(-3.0 / div, 3.0 / div),
            Complex::new(4.0 / div, 2.0 / div),
            Complex::new(5.0 / div, 3.0 / div),
            Complex::new(6.0 / div, -4.0 / div),
            Complex::new(7.0 / div, 5.0 / div),
        ];

        println!("{:.20?}", &channelizer.channelize_fft(&data)[..1]);

        let mut channelizer = Channelizer::new(num_channels);

        println!("{:.20?}", &channelizer.channelize(&data)[..1]);

        let mut channelizer = Channelizer::<i8>::new(num_channels, 4, 1.0);

        let data = [
            Complex::new(0, 1),
            Complex::new(1, 0),
            Complex::new(2, 1),
            Complex::new(-3, 3),
            Complex::new(4, 2),
            Complex::new(5, 3),
            Complex::new(6, -4),
            Complex::new(7, 5),
        ];

        println!("{:.20?}", &channelizer.channelize_fft(&data)[..1]);

        panic!();
    }
    */

    /*
    #[test]
    fn uptest_integer() {
        let num_channels = 16;
        let mut channelizer = Channelizer::<i8>::new(num_channels, 4, 0.75);
        let mut synthesizer = Synthesizer::<f32>::new(num_channels, 4, 0.5);
        let mut synthesizer_liquid = SynthesizerLiquid::new(num_channels);

        fn c(x: i8, y: i8) -> Complex<i8> {
            Complex::new(x, y)
        }
        let data = [
            c(0, 1),
            c(1, 0),
            c(2, 1),
            c(-3, 3),
            c(4, 2),
            c(5, 3),
            c(6, -4),
            c(7, 5),
        ];

        let data_num = 100;
        let mut syn = vec![];
        let mut syn_liquid = vec![];

        for _ in 0..data_num {
            // let channelized = channelizer.channelize_fft(&data);
            let channelized = channelizer.channelize_fft(&data);
            let synthesized = synthesizer.ifft_synthesizer(channelized);
            let synthesized_liquid = synthesizer_liquid.synthesizer(channelized);

            syn.extend_from_slice(&synthesized);
            syn_liquid.extend_from_slice(synthesized_liquid);
        }

        let delay = 2 * num_channels * SYMBOL_DELAY as usize - num_channels / 2 + 1;
        for i in 0..data_num * num_channels {
            let compare = if i < delay {
                Complex::new(0, 0)
            } else {
                data[i - delay]
            };

            println!(
                "{}: syn: {:.15?}, syn_liquid: {:.15?}, compare: {:?}",
                i, syn[i], syn_liquid[i], compare
            );
            // println!("{:.15?}, {:.15?}", syn[i], syn_liquid[i]);
            // rmes += (synthesized[i] - compare).norm_sqr();
        }

        panic!();
    }
    */

    include!("./def_test_data/channelizer.rs");

    #[test]
    #[cfg(not(feature = "channel_power_2"))]
    fn channelize_once() {
        let channel = 20;
        let m = 4;
        let lp_cutoff = 0.75;

        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);

        let mut magic = Channelizer::<i8>::new(channel, m, lp_cutoff);
        let data = (0..10)
            .map(|_| Complex::new(rng.gen(), rng.gen()))
            .collect::<Vec<_>>();

        let result = magic.channelize(&data);

        for (r, e) in result.iter().zip(EXPECT_DATA_CHANNLIZER_ONCE.iter()) {
            assert!(relative_eq!(r, e, epsilon = 1e-6));
        }
    }

    #[test]
    #[cfg(not(feature = "channel_power_2"))]
    fn channelize() {
        let channel = 20;
        let m = 4;
        let lp_cutoff = 0.75;

        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);

        let mut magic = Channelizer::<i8>::new(channel, m, lp_cutoff);
        let data = (0..100)
            .map(|_| Complex::new(rng.gen(), rng.gen()))
            .collect::<Vec<_>>();

        for (chunk, expect) in data.chunks_exact(channel / 2).zip(EXPECT_DATA_CHANNELIZER) {
            let result = magic.channelize(&chunk);

            for (r, e) in result.iter().zip(expect.iter()) {
                if !(relative_eq!(r, e, epsilon = 1e-6)) {
                    panic!("r: {:?}, e: {:?}", r, e);
                }
            }
        }
    }

    #[test]
    fn convert_subfilter() {
        let channel = 3;
        let m = 2;
        let filter = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];

        let filter = filter
            .iter()
            .map(|&x| x as f32 / 32768.0)
            .collect::<Vec<_>>();

        let filter_bank: FilterBank<i8> = FilterBank::from_filter(&filter, channel, m);

        assert_eq!(
            filter_bank.subfilters,
            vec![vec![10, 7, 4, 1], vec![11, 8, 5, 2], vec![12, 9, 6, 3]]
        );
    }

    #[test]
    #[cfg(not(feature = "channel_power_2"))]
    fn convert_subfilter_kaiser_window() {
        let channel = 20;
        let m = 4;
        let filter = generate_kaiser(channel, m, 0.75);

        let filter_bank: FilterBank<i8> = FilterBank::from_filter(&filter, channel, m);

        for (expect, calc) in EXPECT_DATA_FILTER_BANK
            .chunks_exact(2 * m)
            .zip(filter_bank.subfilters.iter())
        {
            for (e, c) in expect.iter().zip(calc.iter()) {
                assert_eq!(*e as i32, *c);
            }
        }
    }

    #[test]
    fn sliding_window() {
        let mut window = SlidingWindow::new(2 * 4);

        for expect in EXPECT_DATA_WINDOW_PUSH {
            let v = Complex::new(expect.v[0] as i32, expect.v[1] as i32);
            window.push(v);

            let r = expect.r.to_vec();
            let i = expect.i.to_vec();

            assert_eq!(window.r.iter().map(|x| x << 8).collect::<Vec<_>>(), r);
            assert_eq!(window.i.iter().map(|x| x << 8).collect::<Vec<_>>(), i);
        }
    }
}

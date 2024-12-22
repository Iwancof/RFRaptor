use liquid_dsp_sys::{firpfbch2_crcf_create_kaiser, LIQUID_ANALYZER, LIQUID_SYNTHESIZER};
use num_complex::Complex;

use crate::liquid::{liquid_do_int, liquid_get_pointer};

pub struct Channelizer {
    num_channels: usize,

    analyzer: core::ptr::NonNull<liquid_dsp_sys::firpfbch2_crcf_s>,

    #[doc(hidden)]
    channel_half: usize,

    #[doc(hidden)]
    working_buffer: Vec<Complex<f32>>,
    // len(working_buffer) = num_channels
}

const SYMBOL_DELAY: u32 = 4;

impl Channelizer {
    pub fn new(num_channels: usize) -> Self {
        let analyzer = liquid_get_pointer(|| unsafe {
            firpfbch2_crcf_create_kaiser(
                LIQUID_ANALYZER as i32,
                num_channels as u32,
                SYMBOL_DELAY,
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

        &self.working_buffer
    }
}

impl Drop for Channelizer {
    fn drop(&mut self) {
        liquid_do_int(|| unsafe { liquid_dsp_sys::firpfbch2_crcf_destroy(self.analyzer.as_ptr()) })
            .expect("firpfbch2_crcf_destroy failed (channelizer)");
    }
}

pub struct Synthesizer {
    synthesizer: core::ptr::NonNull<liquid_dsp_sys::firpfbch2_crcf_s>,
}

#[cfg(test)]
mod test {
    use super::*;
    use approx::relative_eq;
    use num_traits::WrappingAdd;
    use rand::{Rng, SeedableRng};

    use std::simd::*;

    include!("./def_test_data/channelizer.rs");

    #[test]
    fn channelize_once() {
        let channel = 20;

        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);

        let mut magic = Channelizer::new(channel);
        let data = (0..10)
            .map(|_| Complex::new(rng.gen(), rng.gen()))
            .collect::<Vec<_>>();

        let result = magic.channelize(&data);

        for (r, e) in result.iter().zip(EXPECT_DATA_CHANNLIZER_ONCE.iter()) {
            assert!(relative_eq!(r, e, epsilon = 1e-6));
        }
    }

    #[test]
    fn channelize() {
        let channel = 20;

        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);

        let mut magic = Channelizer::new(channel);
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

    extern crate test;

    fn create_mock() -> (Channelizer, Vec<Vec<Complex<i8>>>) {
        let channel = 20;
        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);

        let magic = Channelizer::new(channel);

        let mut data = vec![];
        for _i in 0..100000 {
            let shot = (0..10)
                .map(|_| Complex::new(rng.gen(), rng.gen()))
                .collect::<Vec<_>>();

            data.push(shot);
        }

        (magic, data)
    }
}

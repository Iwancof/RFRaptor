use ice9_bindings::{_pfbch2_t, pfbch2_execute, pfbch2_init};

use num_complex::Complex;

// TODO: FFTを追加する
pub struct Channelizer {
    #[cfg(feature = "ice9")]
    magic: ice9_bindings::_pfbch2_t,

    #[cfg(not(feature = "ice9"))]
    magic: (),

    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
}

impl Channelizer {
    #[cfg(feature = "ice9")]
    pub fn new(channel: usize, m: usize, lp_cutoff: f32) -> Self {
        let mut magic: core::mem::MaybeUninit<_pfbch2_t> = core::mem::MaybeUninit::uninit();
        let mut filter = generate_filter(channel, m, lp_cutoff);

        unsafe {
            pfbch2_init(
                magic.as_mut_ptr(),
                channel as _,
                m as _,
                filter.as_mut_ptr(),
            );
        }

        // SAFETY: filter will be copy into the struct
        // so, we can drop it here

        drop(filter);

        let mut planner = rustfft::FftPlanner::new();

        Self {
            magic: unsafe { magic.assume_init() },
            fft: planner.plan_fft_inverse(channel),
        }
    }

    #[cfg(not(feature = "ice9"))]
    pub fn new(channel: usize, m: usize, lp_cutoff: f32) -> Self {
        Self { magic: () }
    }

    #[cfg(feature = "ice9")]
    pub fn channelize(&mut self, input: &[Complex<i8>]) -> Vec<Complex<f32>> {
        assert_eq!(input.len(), self.magic.M2 as usize);
        let mut output = Vec::with_capacity(self.magic.M2 as usize);

        // SAFETY: Complex<T> has `repr(C)` layout
        let flat_chunk = input.as_ptr() as *mut i8;
        let mut working = [0i16; 96 * 2];

        unsafe {
            pfbch2_execute(
                &mut self.magic as _,
                flat_chunk,
                working.as_mut_ptr() as *mut i16,
            );
        }

        working[..self.magic.M as usize * 2]
            .array_chunks::<2>()
            .for_each(|[re, im]| {
                let re = *re as f32 / 32768.0;
                let im = *im as f32 / 32768.0;
                output.push(Complex::new(re, im));
            });

        output
    }

    pub fn channelize_fft(&mut self, input: &[Complex<i8>]) -> Vec<Complex<f32>> {
        let mut working = self.channelize(input);
        self.fft.process(&mut working);

        working
    }
}

#[cfg(feature = "ice9")]
impl Drop for Channelizer {
    fn drop(&mut self) {
        unsafe {
            ice9_bindings::pfbch2_release(&mut self.magic as _);
        }
    }
}

fn generate_filter(channel: usize, m: usize, lp_cutoff: f32) -> Vec<f32> {
    let h_len = 2 * channel * m + 1;
    let mut buffer = vec![0.0; h_len];

    unsafe {
        liquid_dsp_bindings_sys::liquid_firdes_kaiser(
            h_len as _,
            lp_cutoff / channel as f32,
            60.0,
            0.0,
            buffer.as_mut_ptr(),
        );
    };

    buffer
}

#[cfg(test)]
mod test {
    use super::*;
    use approx::relative_eq;
    use rand::{Rng, SeedableRng};

    include!("./channelizer_define_test.rs");

    #[test]
    fn channelize_once() {
        let channel = 20;
        let m = 4;
        let lp_cutoff = 0.75;

        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);

        let mut magic = Channelizer::new(channel, m, lp_cutoff);
        let data = (0..10)
            .map(|_| Complex::new(rng.gen(), rng.gen()))
            .collect::<Vec<_>>();

        let result = magic.channelize(&data);

        for (r, e) in result.iter().zip(EXPECT_DATA_CHANNLIZER_ONCE.iter()) {
            assert!(relative_eq!(r, e, epsilon = 1e-6));
        }
    }
}

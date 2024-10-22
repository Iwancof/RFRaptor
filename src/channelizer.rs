use ice9_bindings::{_pfbch2_t, pfbch2_execute, pfbch2_init};

use num_complex::Complex;

pub struct Channelizer {
    #[cfg(feature = "ice9")]
    magic: ice9_bindings::_pfbch2_t,

    #[cfg(not(feature = "ice9"))]
    magic: (),
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

        Self {
            magic: unsafe { magic.assume_init() },
        }
    }

    #[cfg(not(feature = "ice9"))]
    pub fn new(channel: usize, m: usize, lp_cutoff: f32) -> Self {
        Self { magic: () }
    }

    #[cfg(feature = "ice9")]
    pub fn channelize(&mut self, input: &[Complex<i8>]) -> Vec<Complex<f32>> {
        assert_eq!(input.len(), 20 / 2);
        let mut output = Vec::with_capacity(20);

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

        working[..20 * 2].array_chunks::<2>().for_each(|[re, im]| {
            let re = *re as f32 / 32768.0;
            let im = *im as f32 / 32768.0;
            output.push(Complex::new(re, im));
        });

        output
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
        let r = liquid_dsp_bindings_sys::liquid_firdes_kaiser(
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

    // #[test]
    // fn test_random
}

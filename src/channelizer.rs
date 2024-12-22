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

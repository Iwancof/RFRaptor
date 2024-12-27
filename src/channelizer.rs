use liquid_dsp_sys::{firpfbch2_crcf_create_kaiser, LIQUID_ANALYZER, LIQUID_SYNTHESIZER};
use num_complex::Complex;

use crate::liquid::{liquid_do_int, liquid_get_pointer};

const SYMBOL_DELAY: u32 = 4;

pub struct Channelizer {
    num_channels: usize,

    analyzer: core::ptr::NonNull<liquid_dsp_sys::firpfbch2_crcf_s>,

    #[doc(hidden)]
    channel_half: usize,

    #[doc(hidden)]
    working_buffer: Box<[Complex<f32>]>,
    // len(working_buffer) = num_channels
}

pub struct Synthesizer {
    num_channels: usize,

    synthesizer: core::ptr::NonNull<liquid_dsp_sys::firpfbch2_crcf_s>,

    #[doc(hidden)]
    channel_half: usize,

    #[doc(hidden)]
    working_buffer: Box<[Complex<f32>]>,
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
                60.0,
            )
        })
        .expect("firpfbch2_crcf_create_kaiser failed (channelizer)");

        Self {
            num_channels,
            channel_half: num_channels / 2,
            analyzer,
            working_buffer: vec![Complex::new(0.0, 0.0); num_channels].into_boxed_slice(),
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

impl Synthesizer {
    pub fn new(num_channels: usize) -> Self {
        let synthesizer = liquid_get_pointer(|| unsafe {
            firpfbch2_crcf_create_kaiser(
                LIQUID_SYNTHESIZER as i32,
                num_channels as u32,
                SYMBOL_DELAY,
                60.0,
            )
        })
        .expect("firpfbch2_crcf_create_kaiser failed (synthesizer)");

        Self {
            num_channels,
            channel_half: num_channels / 2,
            synthesizer,
            working_buffer: vec![Complex::new(0.0, 0.0); num_channels / 2].into_boxed_slice(),
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

        let ((), content) = lent_stdout()
            .unwrap()
            .capture_string(|| {
                unsafe {
                    liquid_dsp_sys::firpfbch2_crcf_print(self.analyzer.as_ptr());
                };
            })
            .unwrap();

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

        let ((), content) = lent_stdout()
            .unwrap()
            .capture_string(|| {
                unsafe {
                    liquid_dsp_sys::firpfbch2_crcf_print(self.synthesizer.as_ptr());
                };
            })
            .unwrap();

        writeln!(f, "Synthesizer")?;
        writeln!(f, "- num_channels: {}", self.num_channels)?;
        writeln!(f, "- synthesizer: {:p}", self.synthesizer)?;

        writeln!(f, "- firpfbch2_crcf_print")?;
        write!(f, "  - {}", content.strip_suffix("\n").unwrap())?;

        Ok(())
    }
}

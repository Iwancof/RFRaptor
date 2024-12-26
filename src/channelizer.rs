use az::WrappingAs;

use num_complex::Complex;
use num_traits::Float;

use crate::liquid::{liquid_do_int, liquid_get_pointer};

const SYMBOL_DELAY: u32 = 4;

/// Channelizer
pub struct Channelizer {
    /// number of channels
    pub num_channels: usize,

    /// filter bank
    filter_bank: FilterBank<i32>,

    /// sliding windows that store the input data
    windows: Vec<SlidingWindow<i32>>,

    /// fft
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,

    #[doc(hidden)]
    channel_half: usize, // num_channels / 2
    //
    #[cfg(feature = "channel_power_2")]
    #[doc(hidden)]
    channel_minus_1: usize,

    #[doc(hidden)]
    flag: bool,

    #[doc(hidden)]
    int_work_buffer: Vec<Complex<i32>>,

    #[doc(hidden)]
    float_work_buffer: Vec<Complex<f32>>,
}

impl Channelizer {
    const SCALE: f32 = 1.0 / 32768.0;

    /// Create a new Channelizer by specifying the number of channels, the number of taps, and the
    /// low-pass cutoff frequency.
    /// This uses a Kaiser window to generate the filter taps internally.
    pub fn new(num_channels: usize, m: usize, lp_cutoff: f32) -> Self {
        if cfg!(feature = "channel_power_2") {
            assert!(num_channels.is_power_of_two());
        }

        let fft = rustfft::FftPlanner::new().plan_fft_inverse(num_channels);
        let windows = (0..num_channels)
            .map(|_| SlidingWindow::new(2 * m))
            .collect::<Vec<_>>();

        Self {
            num_channels,

            #[cfg(feature = "channel_power_2")]
            channel_minus_1: num_channels - 1,

            channel_half: num_channels / 2,

            filter_bank: FilterBank::from_filter(
                &generate_kaiser(num_channels, m, lp_cutoff),
                num_channels,
                m,
            ),

            windows,
            flag: false,
            fft,

            int_work_buffer: Vec::with_capacity(num_channels),
            float_work_buffer: Vec::with_capacity(num_channels),
        }
    }

    fn get_offset(&self) -> usize {
        // Depending on the flag, we use a different window and subfilters.
        if self.flag {
            self.channel_half
        } else {
            0
        }
    }

    // push_to_window explanation:

    // if self.flag == true:
    // [_, _, _, _, ..., push(input[last]), push(input[last-1]), ..., push(input[0])]
    //                   ^ half of the channel
    //
    // if self.flag == false:
    // [push(input[last]), push(input[last-1]), ..., push(input[0]), _, _, _, _, ...]
    //                                                               ^ half of the channel

    #[allow(unused)]
    fn push_to_window(&mut self, input: &[Complex<i8>]) {
        if self.flag {
            for (i, data) in input.iter().enumerate() {
                let &Complex { re, im } = data;
                let data = Complex::new(re as i32, im as i32);

                let window_idx = self.num_channels - i - 1;

                // SAFETY: if input.len() is smaller than self.channel_half.
                unsafe {
                    self.windows.get_unchecked_mut(window_idx).push(data);
                }
            }
        } else {
            for (i, data) in input.iter().enumerate() {
                let &Complex { re, im } = data;
                let data = Complex::new(re as i32, im as i32);

                let window_idx = self.channel_half - i - 1;

                // self.windows[window_idx].push(*idx);
                unsafe {
                    self.windows.get_unchecked_mut(window_idx).push(data);
                }
            }
        }
    }

    #[allow(unused)]
    pub fn apply(&mut self) {
        let offset = self.get_offset();

        self.float_work_buffer.clear();
        for (ch_idx, window) in self.windows.iter_mut().enumerate() {
            #[cfg(feature = "channel_power_2")]
            let current_pos = (offset + ch_idx) & self.channel_minus_1;
            #[cfg(not(feature = "channel_power_2"))]
            let current_pos = (offset + ch_idx) % self.num_channels;

            let sf = &self.filter_bank.subfilters[current_pos];

            let Complex { re, im } = window.apply_filter(sf);
            self.float_work_buffer.push(Complex::new(
                (re >> 8) as f32 * Self::SCALE,
                (im >> 8) as f32 * Self::SCALE,
            ));
        }
    }

    /// Channelize the input data.
    /// The input data must be exactly half the number of channels.
    /// Length of the output data is the same as the number of channels.
    pub fn channelize(&mut self, input: &[Complex<i8>]) -> &mut Vec<Complex<f32>> {
        debug_assert_eq!(input.len(), self.channel_half);

        self.push_to_window(input);
        self.apply();

        self.flag = !self.flag;

        &mut self.float_work_buffer
    }

    /// Channelize the input data and perform an FFT.
    pub fn channelize_fft(&mut self, input: &[Complex<i8>]) -> &mut Vec<Complex<f32>> {
        self.channelize(input);
        self.fft.process(&mut self.float_work_buffer);

        &mut self.float_work_buffer
    }
}

impl core::fmt::Debug for Channelizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channelizer")
            .field("num_channels", &self.num_channels)
            .field("channel_half", &self.channel_half)
            .field("filter_bank", &self.filter_bank)
            .field("windows", &self.windows)
            .field("flag", &self.flag)
            .finish()
    }
}

trait FilterCell {
    fn from_f32(f: f32) -> Self;
    fn to_f32(&self) -> f32;
}

impl FilterCell for f32 {
    #[inline]
    fn from_f32(f: f32) -> Self {
        f
    }

    #[inline]
    fn to_f32(&self) -> f32 {
        *self
    }
}

impl FilterCell for i32 {
    #[inline]
    fn from_f32(f: f32) -> Self {
        (f * 32768.0).round() as i32
    }

    #[inline]
    fn to_f32(&self) -> f32 {
        const SCALE: f32 = 1.0 / 32768.0;
        *self as f32 * SCALE
    }
}

/// Filter bank
#[derive(Debug)]
pub struct FilterBank<T: FilterCell> {
    subfilters: Vec<Vec<T>>,
    // subfilters.len() == channels;
    // subfilters[forall n].len() is subfilter length
    //
    // subfilters[forall n] is reversed filter
}

impl<T> FilterBank<T>
where
    T: FilterCell + Default + Clone + Copy,
{
    /// Create a new FilterBank from the given filter taps.
    fn from_filter(filter: &[f32], num_channels: usize, m: usize) -> Self {
        let subfilter_length = 2 * m;

        assert_eq!(filter.len(), subfilter_length * num_channels + 1);

        // STEP1: make `filter`'s type to i16
        let filter = filter
            .iter()
            // .map(|&x| ((x * 32768.0).round() as i32).wrapping_as::<i16>() as i32)
            .map(|&x| T::from_f32(x))
            .collect::<Vec<_>>();

        // STEP2: split `filter` into subfilters
        let mut subfilters = vec![vec![T::default(); subfilter_length]; num_channels];
        for (pos, filter_fragment) in filter.chunks_exact(num_channels).enumerate() {
            for ch_idx in 0..num_channels {
                subfilters[ch_idx][pos] = filter_fragment[ch_idx];
            }
        }

        // STEP3: reverse subfilters
        for subfilter in subfilters.iter_mut() {
            subfilter.reverse();
            // this makes convolution easier(dot product)
        }

        Self { subfilters }
    }
}

/// Sliding window
#[derive(Debug)]
pub struct SlidingWindow<T: Default + Clone + Copy> {
    pub current_pos: usize,
    pub len: usize,
    pub offset: usize,

    pub r: Vec<T>,
    pub i: Vec<T>,
}

impl<T> SlidingWindow<T>
where
    T: Default + Clone + Copy,
{
    pub(crate) fn new(len: usize) -> Self {
        assert!(len.is_power_of_two());

        let offset = 2 * len;

        Self {
            current_pos: 0,
            len,
            offset,
            r: vec![T::default(); len + offset - 1],
            i: vec![T::default(); len + offset - 1],
        }
    }

    pub(crate) fn push(&mut self, data: Complex<T>) {
        let Complex { re, im } = data;

        self.current_pos += 1;
        self.current_pos &= self.offset - 1;

        if self.current_pos == 0 {
            self.r.copy_within(self.offset.., 0);
            self.i.copy_within(self.offset.., 0);
        }

        let write_pos = self.current_pos + self.len - 1; // TODO: remove overflow check

        // self.r[write_pos] = re as i32;
        // self.i[write_pos] = im as i32;

        unsafe {
            // remove overflow check
            *self.r.get_unchecked_mut(write_pos) = re;
            *self.i.get_unchecked_mut(write_pos) = im;
        }
    }
}

impl SlidingWindow<i32> {
    pub(crate) fn apply_filter(&self, filter: &[i32]) -> Complex<i32> {
        debug_assert_eq!(filter.len(), self.len);

        debug_assert_eq!(self.len, 8); // FIXME: remove this constraint

        #[link(name = "apply_filter", kind = "static")]
        extern "C" {
            fn dotprod_8(r: *const i32, i: *const i32, h: *const i32, out: *mut i32);
            // implemented in src/apply_filter.c
        }

        let mut out = [0i32; 2];
        unsafe {
            // FIXME: replace with std::simd
            dotprod_8(
                self.r.as_ptr().add(self.current_pos),
                self.i.as_ptr().add(self.current_pos),
                filter.as_ptr(),
                out.as_mut_ptr(),
            );
        }

        Complex::new(out[0], out[1])
    }
}

fn generate_kaiser(channel: usize, m: usize, lp_cutoff: f32) -> Vec<f32> {
    let h_len = 2 * channel * m + 1;
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

    buffer
}

#[cfg(test)]
mod test {
    use super::*;
    use approx::relative_eq;
    use num_traits::WrappingAdd;
    use rand::{rngs::SmallRng, Rng, SeedableRng};

    use std::simd::*;

    include!("./def_test_data/channelizer.rs");

    #[test]
    #[cfg(not(feature = "channel_power_2"))]
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

    #[test]
    #[cfg(not(feature = "channel_power_2"))]
    fn channelize() {
        let channel = 20;
        let m = 4;
        let lp_cutoff = 0.75;

        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);

        let mut magic = Channelizer::new(channel, m, lp_cutoff);
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

        let filter_bank: FilterBank<i32> = FilterBank::from_filter(&filter, channel, m);

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

        let filter_bank = FilterBank::from_filter(&filter, channel, m);

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

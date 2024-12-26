use core::fmt::Debug;
use std::default;

use az::WrappingAs;

use liquid_dsp_sys::{firpfbch2_crcf_create_kaiser, LIQUID_SYNTHESIZER};
use num_complex::Complex;
use num_traits::Float;

use crate::liquid::{liquid_do_int, liquid_get_pointer};

const SYMBOL_DELAY: u32 = 4;

pub trait Signal: Debug + Default + Copy + num_traits::Num {
    type InternalRepr: Debug + Default + Copy + num_traits::Num;

    fn from_internal(rs: Self::InternalRepr) -> Self;
    fn to_internal(sg: Self) -> Self::InternalRepr;

    fn from_f32(f: f32) -> Self::InternalRepr;
    fn to_f32(sg: Self::InternalRepr) -> f32;
}

impl Signal for f32 {
    type InternalRepr = f32;

    fn from_internal(rs: Self::InternalRepr) -> Self {
        rs
    }
    fn to_internal(sg: Self) -> Self::InternalRepr {
        sg
    }
    fn from_f32(f: f32) -> Self::InternalRepr {
        f
    }
    fn to_f32(sg: Self::InternalRepr) -> f32 {
        sg
    }
}

impl Signal for i8 {
    type InternalRepr = i32;

    fn from_internal(rs: Self::InternalRepr) -> Self {
        debug_assert!(rs >= -128 && rs <= 127);

        rs as i8
    }
    fn to_internal(sg: Self) -> Self::InternalRepr {
        sg as i32
    }
    fn from_f32(f: f32) -> Self::InternalRepr {
        // TODO: fix this with simply casting
        // ((f * 32768.0).round() as i32).wrapping_as::<i16>() as i32
        (f * 32768.0).round() as i32
    }
    fn to_f32(sg: Self::InternalRepr) -> f32 {
        const SCALE: f32 = 1.0 / 32768.0;
        sg as f32 * SCALE
    }
}

fn complex_map<T, U, F>(a: Complex<T>, f: F) -> Complex<U>
where
    F: Fn(T) -> U,
{
    Complex::new(f(a.re), f(a.im))
}

/// Channelizer
pub struct Channelizer<S: Signal> {
    /// number of channels
    pub num_channels: usize,

    /// filter bank
    filter_bank: FilterBank<S>,

    /// sliding windows that store the input data
    windows: Vec<SlidingWindow<S::InternalRepr>>,

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
    float_work_buffer: Vec<Complex<f32>>,
}

impl<S> Channelizer<S>
where
    S: Signal,
{
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
    fn push_to_window(&mut self, input: &[Complex<S>]) {
        if self.flag {
            for (i, data) in input.iter().enumerate() {
                // let &Complex { re, im } = data;
                // let data = Complex::new(re as Input, im as i32);

                let window_idx = self.num_channels - i - 1;

                // SAFETY: if input.len() is smaller than self.channel_half.
                // unsafe {
                //     self.windows.get_unchecked_mut(window_idx).push(data);
                // }
                self.windows[window_idx].push(complex_map(*data, S::to_internal));
            }
        } else {
            for (i, data) in input.iter().enumerate() {
                // let &Complex { re, im } = data;
                // let data = Complex::new(re as i32, im as i32);

                let window_idx = self.channel_half - i - 1;

                // self.windows[window_idx].push(*idx);
                // unsafe {
                //     self.windows.get_unchecked_mut(window_idx).push(data);
                // }
                self.windows[window_idx].push(complex_map(*data, S::to_internal));
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
            self.float_work_buffer
                // .push(Complex::new((re >> 8).to_f32(), (im >> 8).to_f32()));
                // .push(Complex::new((re).to_f32(), (im).to_f32()));
                .push(complex_map(Complex::new(re, im), S::to_f32));
        }
    }

    /// Channelize the input data.
    /// The input data must be exactly half the number of channels.
    /// Length of the output data is the same as the number of channels.
    pub fn channelize(&mut self, input: &[Complex<S>]) -> &mut Vec<Complex<f32>> {
        debug_assert_eq!(input.len(), self.channel_half);

        self.push_to_window(input);
        self.apply();

        self.flag = !self.flag;

        &mut self.float_work_buffer
    }

    /// Channelize the input data and perform an FFT.
    pub fn channelize_fft(&mut self, input: &[Complex<S>]) -> &mut Vec<Complex<f32>> {
        self.channelize(input);
        self.fft.process(&mut self.float_work_buffer);

        for x in self.float_work_buffer.iter_mut() {
            x.re /= self.num_channels as f32 * 256.;
            x.im /= self.num_channels as f32 * 256.;
        }

        &mut self.float_work_buffer
    }
}

impl<T> core::fmt::Debug for Channelizer<T>
where
    T: Signal + Debug + Default + Copy,
{
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

/// Synthesizer
pub struct Synthesizer<S: Signal> {
    /// number of channels
    pub num_channels: usize,

    /// filter bank
    filter_bank: FilterBank<S>,

    /// window
    window_0: Vec<SlidingWindow<S::InternalRepr>>,
    window_1: Vec<SlidingWindow<S::InternalRepr>>,

    /// fft
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,

    #[doc(hidden)]
    channel_half: usize, // num_channels / 2

    #[cfg(feature = "channel_power_2")]
    #[doc(hidden)]
    channel_minus_1: usize,

    #[doc(hidden)]
    flag: bool,

    #[doc(hidden)]
    float_work_buffer: Vec<Complex<f32>>,
}

impl<S> Synthesizer<S>
where
    S: Signal,
{
    pub fn new(num_channels: usize, m: usize, lp_cutoff: f32) -> Self {
        if cfg!(feature = "channel_power_2") {
            assert!(num_channels.is_power_of_two());
        }

        let fft = rustfft::FftPlanner::new().plan_fft_inverse(num_channels);
        let window_0 = (0..num_channels)
            .map(|_| SlidingWindow::new(2 * m))
            .collect::<Vec<_>>();
        let window_1 = (0..num_channels)
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

            window_0,
            window_1,
            flag: false,
            fft,

            float_work_buffer: Vec::with_capacity(num_channels),
        }
    }

    pub fn ifft_synthesizer(&mut self, input: &[Complex<f32>]) -> Vec<Complex<S>> {
        // pub fn ifft_synthesizer(&mut self, input: &[Complex<f32>]) -> Vec<Complex<f32>> {
        debug_assert_eq!(input.len(), self.num_channels);

        input.clone_into(&mut self.float_work_buffer);
        self.fft.process(&mut self.float_work_buffer);

        // scale it
        for x in self.float_work_buffer.iter_mut() {
            x.re /= self.num_channels as f32;
            x.im /= self.num_channels as f32;
        }

        for x in self.float_work_buffer.iter_mut() {
            x.re *= self.channel_half as f32;
            x.im *= self.channel_half as f32;
        }

        let dest_window = if self.flag {
            &mut self.window_0
        } else {
            &mut self.window_1
        };

        for (i, d) in self.float_work_buffer.iter().enumerate() {
            dest_window[i].push(complex_map(*d, S::from_f32));
        }

        let mut tmp = vec![];
        if self.flag {
            for start_pos in 0..self.channel_half {
                let a = self.window_0[start_pos + self.channel_half]
                    .apply_filter(&self.filter_bank.subfilters[start_pos]);
                let b = self.window_1[start_pos + self.channel_half]
                    .apply_filter(&self.filter_bank.subfilters[start_pos + self.channel_half]);
                tmp.push(complex_map(a + b, S::from_internal));
            }
        } else {
            for start_pos in 0..self.channel_half {
                let a = self.window_0[start_pos]
                    .apply_filter(&self.filter_bank.subfilters[start_pos + self.channel_half]);
                let b =
                    self.window_1[start_pos].apply_filter(&self.filter_bank.subfilters[start_pos]);
                tmp.push(complex_map(a + b, S::from_internal));
            }
        }

        self.flag = !self.flag;

        tmp
    }
}

/// Filter bank
#[derive(Debug)]
pub struct FilterBank<S: Signal> {
    subfilters: Vec<Vec<S::InternalRepr>>,
    // subfilters.len() == channels;
    // subfilters[forall n].len() is subfilter length
    //
    // subfilters[forall n] is reversed filter
}

impl<T> FilterBank<T>
where
    T: Signal + Default + Clone + Copy,
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
        let mut subfilters = vec![vec![T::InternalRepr::default(); subfilter_length]; num_channels];
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

trait FilterApplicable<T> {
    fn apply_filter(&self, filter: &[T]) -> Complex<T>;
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

        self.r[write_pos] = re;
        self.i[write_pos] = im;

        // unsafe {
        //     // remove overflow check
        //     *self.r.get_unchecked_mut(write_pos) = re;
        //     *self.i.get_unchecked_mut(write_pos) = im;
        // }
    }
}

impl<T> FilterApplicable<T> for SlidingWindow<T>
where
    T: Default + Copy,
{
    default fn apply_filter(&self, _filter: &[T]) -> Complex<T> {
        unimplemented!()
    }
}

impl FilterApplicable<i32> for SlidingWindow<i32> {
    #[cfg(not(debug_assertions))]
    fn apply_filter(&self, filter: &[i32]) -> Complex<i32> {
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

        let out = Complex::new(out[0], out[1]);
        // complex_map(out, | x | x >> 8)
        out
    }

    #[cfg(debug_assertions)]
    fn apply_filter(&self, filter: &[i32]) -> Complex<i32> {
        debug_assert_eq!(filter.len(), self.len);

        let mut out = Complex::new(0, 0);
        for (i, &f) in filter.iter().enumerate() {
            out.re += self.r[self.current_pos + i] * f;
            out.im += self.i[self.current_pos + i] * f;
        }

        out
        // complex_map(out, | x | x >> 8)
    }
}

impl FilterApplicable<f32> for SlidingWindow<f32> {
    fn apply_filter(&self, filter: &[f32]) -> Complex<f32> {
        debug_assert_eq!(filter.len(), self.len);

        let mut out = Complex::new(0.0, 0.0);
        for (i, &f) in filter.iter().enumerate() {
            out.re += self.r[self.current_pos + i] as f32 * f;
            out.im += self.i[self.current_pos + i] as f32 * f;
        }

        out
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

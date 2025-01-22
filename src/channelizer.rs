use az::WrappingAs;

use num_complex::Complex;

/// Channelizer
pub struct Channelizer {
    /// number of channels
    pub num_channels: usize,

    /// filter bank
    filter_bank: FilterBank,

    /// sliding windows that store the input data
    windows: Vec<SlidingWindow>,

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
    fn push_to_window_1(&mut self, input: &[Complex<i8>]) {
        debug_assert_eq!(input.len(), self.channel_half);

        let offset = self.get_offset();

        for (window, x) in self.windows[offset..]
            .iter_mut()
            .take(self.channel_half)
            .rev()
            .zip(input)
        {
            window.push(*x);
        }
    }

    #[allow(unused)]
    fn push_to_window_2(&mut self, input: &[Complex<i8>]) {
        if self.flag {
            for (i, idx) in input.iter().enumerate() {
                let window_idx = self.num_channels - i - 1;

                // SAFETY: if input.len() is smaller than self.channel_half.
                unsafe {
                    self.windows.get_unchecked_mut(window_idx).push(*idx);
                }
            }
        } else {
            for (i, idx) in input.iter().enumerate() {
                let window_idx = self.channel_half - i - 1;

                // self.windows[window_idx].push(*idx);
                unsafe {
                    self.windows.get_unchecked_mut(window_idx).push(*idx);
                }
            }
        }
    }

    #[allow(unused)]
    pub fn apply_1(&mut self) {
        let offset = self.get_offset();

        self.int_work_buffer.clear();
        for (ch_idx, window) in self.windows.iter_mut().enumerate() {
            #[cfg(feature = "channel_power_2")]
            let current_pos = (offset + ch_idx) & self.channel_minus_1;
            #[cfg(not(feature = "channel_power_2"))]
            let current_pos = (offset + ch_idx) % self.num_channels;

            let sf = &self.filter_bank.subfilters[current_pos];

            self.int_work_buffer.push(window.apply_filter(sf)); // apply kaiser window
        }

        self.float_work_buffer.clear();
        for Complex { re, im } in self.int_work_buffer.iter() {
            self.float_work_buffer.push(Complex::new(
                (*re as f32) * Self::SCALE,
                (*im as f32) * Self::SCALE,
            ));
        }
    }

    #[allow(unused)]
    pub fn apply_2(&mut self) {
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
                re as f32 * Self::SCALE,
                im as f32 * Self::SCALE,
            ));
        }
    }

    #[allow(unused)]
    pub fn apply_3(&mut self) {
        let offset = self.get_offset();

        self.float_work_buffer.clear();
        for (ch_idx, window) in self.windows.iter_mut().enumerate() {
            #[cfg(feature = "channel_power_2")]
            let current_pos = (offset + ch_idx) & self.channel_minus_1;
            #[cfg(not(feature = "channel_power_2"))]
            let current_pos = (offset + ch_idx) % self.num_channels;

            let sf = &self.filter_bank.subfilters[current_pos];

            self.float_work_buffer.push(window.apply_filter_float(sf)); // apply kaiser window
        }
    }

    /// Channelize the input data.
    /// The input data must be exactly half the number of channels.
    pub fn channelize(&mut self, input: &[Complex<i8>]) -> &mut Vec<Complex<f32>> {
        debug_assert_eq!(input.len(), self.channel_half);

        self.push_to_window_2(input);
        self.apply_2();

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

/// Filter bank
#[derive(Debug)]
pub struct FilterBank {
    subfilters: Vec<Vec<i32>>,
    // subfilters.len() == channels;
    // subfilters[forall n].len() is subfilter length
    //
    // subfilters[forall n] is reversed filter
}

impl FilterBank {
    /// Create a new FilterBank from the given filter taps.
    fn from_filter(filter: &[f32], num_channels: usize, m: usize) -> Self {
        let subfilter_length = 2 * m;

        assert_eq!(filter.len(), subfilter_length * num_channels + 1);

        // STEP1: make `filter`'s type to i16
        let filter = filter
            .iter()
            .map(|&x| ((x * 32768.0).round() as i32).wrapping_as::<i16>() as i32)
            .collect::<Vec<_>>();

        // STEP2: split `filter` into subfilters
        let mut subfilters = vec![vec![0; subfilter_length]; num_channels];
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
pub struct SlidingWindow {
    pub current_pos: usize,
    pub len: usize,
    pub offset: usize,

    // TODO: use Vec<Complex<i16>> instead of Vec<i16>
    pub r: Vec<i32>,
    pub i: Vec<i32>,
}

impl SlidingWindow {
    pub(crate) fn new(len: usize) -> Self {
        assert!(len.is_power_of_two());

        let offset = 2 * len;

        Self {
            current_pos: 0,
            len,
            offset,
            r: vec![0; len + offset - 1],
            i: vec![0; len + offset - 1],
        }
    }

    pub(crate) fn push(&mut self, data: Complex<i8>) {
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
            *self.r.get_unchecked_mut(write_pos) = re as i32;
            *self.i.get_unchecked_mut(write_pos) = im as i32;
        }
    }

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

        Complex::new(out[0] >> 8, out[1] >> 8) // due to sdr's signal format
    }

    pub(crate) fn apply_filter_float(&self, filter: &[i32]) -> Complex<f32> {
        debug_assert_eq!(filter.len(), self.len);
        debug_assert_eq!(self.len, 8); // FIXME: remove this constraint

        #[link(name = "apply_filter", kind = "static")]
        extern "C" {
            fn dotprod_8_float(r: *const i32, i: *const i32, h: *const i32, out: *mut f32);
        }

        let mut out: [f32; 2] = [0.0; 2];

        unsafe {
            dotprod_8_float(
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

pub struct Synthesizer {}

#[cfg(test)]
mod test {
    use super::*;
    use approx::relative_eq;
    use num_traits::WrappingAdd;
    use rand::{Rng, SeedableRng};

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

        let filter_bank = FilterBank::from_filter(&filter, channel, m);

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
            let v = Complex::new(expect.v[0], expect.v[1]);
            window.push(v);

            let r = expect.r.to_vec();
            let i = expect.i.to_vec();

            assert_eq!(window.r.iter().map(|x| x << 8).collect::<Vec<_>>(), r);
            assert_eq!(window.i.iter().map(|x| x << 8).collect::<Vec<_>>(), i);
        }
    }

    extern crate test;

    #[bench]
    fn channelize_fft_100000(b: &mut test::Bencher) {
        let channel = 16;
        let m = 4;
        let lp_cutoff = 0.75;

        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);

        let mut magic = Channelizer::new(channel, m, lp_cutoff);
        let data = (0..16 * 100000)
            .map(|_| Complex::new(rng.gen(), rng.gen()))
            .collect::<Vec<_>>();

        b.iter(|| {
            for chunk in data.chunks_exact(channel / 2) {
                magic.channelize_fft(chunk);
            }
        });
    }
}

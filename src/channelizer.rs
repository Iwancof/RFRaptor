use az::WrappingAs;

use num_complex::Complex;

pub struct Channelizer {
    num_channels: usize,
    channel_half: usize,
    filter_bank: FilterBank,
    windows: Vec<SlidingWindow>,
    flag: bool,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
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

#[derive(Debug)]
pub struct FilterBank {
    subfilters: Vec<Vec<i16>>,
    // subfilters.len() == channels;
    // subfilters[forall n].len() is subfilter length
    //
    // subfilters[forall n] is reversed filter
}

#[derive(Debug)]
pub struct SlidingWindow {
    pub current_pos: usize,
    pub len: usize,
    pub offset: usize,

    // TODO: use Vec<Complex<i16>> instead of Vec<i16>
    pub r: Vec<i16>,
    pub i: Vec<i16>,
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

        let write_pos = self.current_pos + self.len - 1;
        self.r[write_pos] = (re as i16) << 8i16;
        self.i[write_pos] = (im as i16) << 8i16;
    }

    pub(crate) fn apply_filter(&self, filter: &[i16]) -> Complex<i16> {
        assert_eq!(filter.len(), self.len);

        let mut re = 0i32;
        let mut im = 0i32;

        for (x, h) in self.r[self.current_pos..].iter().zip(filter) {
            re += *x as i32 * *h as i32;
        }

        for (x, h) in self.i[self.current_pos..].iter().zip(filter) {
            im += *x as i32 * *h as i32;
        }

        Complex::new((re >> 16) as i16, (im >> 16) as i16)
    }
}

impl FilterBank {
    fn from_filter(filter: &[f32], num_channels: usize, m: usize) -> Self {
        let subfilter_length = 2 * m;

        assert_eq!(filter.len(), subfilter_length * num_channels + 1); // TODO: check length

        // STEP1: make `filter`'s type to i16
        let filter = filter
            .iter()
            .map(|&x| {
                let ret = ((x * 32768.0).round() as i32).wrapping_as::<i16>();

                ret
            })
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
        }

        Self { subfilters }
    }
}

impl Channelizer {
    pub fn new(num_channels: usize, m: usize, lp_cutoff: f32) -> Self {
        let fft = rustfft::FftPlanner::new().plan_fft_inverse(num_channels);
        let windows = (0..num_channels)
            .map(|_| SlidingWindow::new(2 * m))
            .collect::<Vec<_>>();

        Self {
            num_channels,
            channel_half: num_channels / 2,
            filter_bank: FilterBank::from_filter(
                &generate_filter(num_channels, m, lp_cutoff),
                num_channels,
                m,
            ),
            windows,
            flag: false,
            fft,
        }
    }

    pub fn channelize(&mut self, input: &[Complex<i8>]) -> Vec<Complex<f32>> {
        assert_eq!(input.len(), self.channel_half);

        let offset = if self.flag { self.channel_half } else { 0 };

        for (window, x) in self.windows[offset..]
            .iter_mut()
            .take(self.channel_half)
            .rev()
            .zip(input)
        {
            window.push(*x);
        }

        // if self.flag == true:
        // [_, _, _, _, ..., push(input[last]), push(input[last-1]), ..., push(input[0])]
        //                   ^ half of the channel
        //
        // if self.flag == false:
        // [push(input[last]), push(input[last-1]), ..., push(input[0]), _, _, _, _, ...]
        //                                                               ^ half of the channel

        let mut output = Vec::with_capacity(self.channel_half);

        for (ch_idx, window) in self.windows.iter_mut().enumerate() {
            let current_pos = (offset + ch_idx) % self.num_channels;
            let sf = &self.filter_bank.subfilters[current_pos];

            output.push(window.apply_filter(sf));
        }

        let output = output
            .iter()
            .map(|x| Complex::new(x.re as f32 / 32768.0, x.im as f32 / 32768.0))
            .collect::<Vec<_>>();

        self.flag = !self.flag;

        output
    }

    pub fn channelize_fft(&mut self, input: &[Complex<i8>]) -> Vec<Complex<f32>> {
        let mut working = self.channelize(input);
        self.fft.process(&mut working);

        working
    }
}

fn generate_filter(channel: usize, m: usize, lp_cutoff: f32) -> Vec<f32> {
    let h_len = 2 * channel * m + 1;
    let mut buffer = Vec::with_capacity(h_len);

    unsafe {
        liquid_dsp_bindings_sys::liquid_firdes_kaiser(
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

    #[test]
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
    fn convert_subfilter_kaiser_window() {
        let channel = 20;
        let m = 4;
        let filter = generate_filter(channel, m, 0.75);

        let filter_bank = FilterBank::from_filter(&filter, channel, m);

        for (expect, calc) in EXPECT_DATA_FILTER_BANK
            .chunks_exact(2 * m)
            .zip(filter_bank.subfilters.iter())
        {
            for (e, c) in expect.iter().zip(calc.iter()) {
                assert_eq!(*e, *c);
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

            assert_eq!(window.r, r);
            assert_eq!(window.i, i);
        }
    }
}

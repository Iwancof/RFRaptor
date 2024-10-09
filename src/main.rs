#![feature(iter_array_chunks)]
#![feature(core_intrinsics)]
#![feature(array_chunks)]

use core::fmt;

use anyhow::Context;
use soapysdr::Direction::Rx;

use core::mem::MaybeUninit;

use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

#[log_derive::logfn(ok = "TRACE", err = "ERROR")]
fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    soapysdr::configure_logging();

    let filter = "hackrf";
    log::trace!("filter is {}", filter);

    let devarg = soapysdr::enumerate(filter)
        .context("failed to enumerate devices")?
        .into_iter()
        .next()
        .context("No devices found")?;
    log::trace!("found device {}", devarg);

    let dev = soapysdr::Device::new(devarg)?;

    let config = SDRConfig {
        channels: 0,
        center_freq: 2426.0e6, // bluetooth
        sample_rate: 20.0e6,
        bandwidth: 20.0e6,
        gain: 20.0,
    };

    log::info!("config = {}", config);
    config.set(&dev)?;

    let mut magic: MaybeUninit<ice9_bindings::pfbch2_t> = MaybeUninit::uninit();

    // initialize ice9 bindings
    unsafe {
        use ice9_bindings::*;

        let channel = 20;
        let m = 4;
        let lp_cutoff = 0.75;

        let h_len = 2 * channel * m + 1;
        let mut buffer = vec![0.0; h_len].into_boxed_slice();

        liquid_dsp_bindings_sys::liquid_firdes_kaiser(
            h_len as _,
            lp_cutoff / channel as f32,
            60.0,
            0.0,
            buffer.as_mut_ptr(),
        );

        pfbch2_init(
            magic.as_mut_ptr(),
            channel as _,
            m as _,
            buffer.as_mut_ptr(),
        );
    }

    let mut magic = unsafe { magic.assume_init() };
    println!("{:?}", magic);

    let mut stream = dev.rx_stream::<num_complex::Complex<i8>>(&[config.channels])?;
    // let mut _write_stream = dev.tx_stream::<num_complex::Complex<u8>>(&[config.channels])?;

    let sb = signalbool::SignalBool::new(&[signalbool::Signal::SIGINT], signalbool::Flag::Restart)?;

    let mut buffer = vec![num_complex::Complex::<i8>::new(0, 0); stream.mtu()?];
    let mut burst = CRCF::new();

    stream.activate(None)?;

    const BATCH_SIZE: usize = 4096;

    use std::io::{BufWriter, Write};
    let mut file = BufWriter::new(std::fs::File::create("signal.dat")?);

    let mut counter = 0;

    '_outer: for _ in 0.. {
        let read = stream.read(&mut [&mut buffer[..]], 1_000_000)?;
        assert_eq!(read, buffer.len());

        let mut channel_0 = Vec::with_capacity(BATCH_SIZE);
        for chunk in buffer.chunks_mut(20 / 2) {
            let mut output = vec![0i16; 96 * 2];

            unsafe {
                // SAFETY: Complex<T> has `repr(C)` layout
                let flat_chunk = chunk.as_mut_ptr() as *mut i8;

                ice9_bindings::pfbch2_execute(
                    &mut magic as _,
                    flat_chunk,
                    output.as_mut_ptr() as *mut i16,
                );
            }

            let buf = output[..20 * 2]
                .array_chunks::<2>()
                .map(|reim_pair| {
                    let [re, im] = reim_pair;

                    let re = *re as f32 / 32768.0;
                    let im = *im as f32 / 32768.0;

                    num_complex::Complex::new(re, im)
                })
                .collect::<Vec<_>>();

            channel_0.push(buf[9]);

            if channel_0.len() == BATCH_SIZE {
                let mut planner = rustfft::FftPlanner::new();
                let fft = planner.plan_fft_inverse(4096);

                fft.process(&mut channel_0);

                for e in &channel_0 {
                    // match burst.catcher(*e / 20 as f32) as _ {
                    let (sample, status) = burst.execute(*e / 20 as f32);
                    match status as _ {
                        //liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_RISE => {
                        //    // println!("LIQUID_AGC_SQUELCH_RISE: {:?}", sample);
                        //}
                        liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_SIGNALHI => {
                            // println!("LIQUID_AGC_SQUELCH_SIGNALHI");
                            writeln!(file, "{} {} {}", counter, sample.norm(), 0)?;
                        }
                        //liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_TIMEOUT => {
                        //    // println!("LIQUID_AGC_SQUELCH_TIMEOUT");
                        //}
                        _ => {
                            writeln!(file, "{} {} {}", counter, sample.norm(), 1)?;
                        }
                    }

                    counter += 1;
                }

                /*
                use std::io::{BufWriter, Write};

                let mut file = BufWriter::new(std::fs::File::create("output.dat")?);

                // translate this python code `np.abs(np.fft.fft(x))**2 / (N*Fs)`
                let power = channel_0.iter().map(|x| x.norm_sqr()).collect::<Vec<_>>();
                let power = power
                    .iter()
                    .map(|x| x / (BATCH_SIZE as f32 * config.sample_rate as f32))
                    .collect::<Vec<_>>();

                // make it log scale
                let power = power.iter().map(|x| 10.0 * x.log10()).collect::<Vec<_>>();

                let freq_step = config.sample_rate / BATCH_SIZE as f64;

                // shift the zero frequency to the center
                let mut power = power
                    .iter()
                    .enumerate()
                    .map(|(i, x)| {
                        let i = if i < BATCH_SIZE / 2 {
                            i as isize
                        } else {
                            i as isize - BATCH_SIZE as isize
                        };
                        (i, x)
                    })
                    .collect::<Vec<_>>();

                // sort by frequency
                power.sort_by(|a, b| a.0.cmp(&b.0));

                // convert to frequency
                let power = power
                    .iter()
                    .map(|(i, x)| (*i as f64 * freq_step, *x))
                    .collect::<Vec<_>>();

                for p in power.iter() {
                    writeln!(file, "{} {}", p.0, p.1)?;
                }
                */

                // break '_outer;
            }
        }

        if sb.caught() {
            break;
        }
    }

    stream.deactivate(None)?;

    Ok(())
}

struct SDRConfig {
    channels: usize,
    center_freq: f64,
    sample_rate: f64,
    bandwidth: f64,
    gain: f64,
}

impl SDRConfig {
    fn set(&self, dev: &soapysdr::Device) -> anyhow::Result<()> {
        for channel in 0..self.channels {
            dev.set_frequency(Rx, channel, self.center_freq, ())?;
            dev.set_sample_rate(Rx, channel, self.sample_rate)?;
            dev.set_bandwidth(Rx, channel, self.bandwidth)?;
            dev.set_gain(Rx, channel, self.gain)?;
        }
        Ok(())
    }
}

impl fmt::Display for SDRConfig {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "channels: {}, center_freq: {}, sample_rate: {}, bandwidth: {}, gain: {}",
            self.channels, self.center_freq, self.sample_rate, self.bandwidth, self.gain
        )
    }
}

struct CRCF {
    crcf: liquid_dsp_bindings_sys::agc_crcf,
}

impl CRCF {
    pub fn new() -> Self {
        use liquid_dsp_bindings_sys::*;
        let crcf = unsafe {
            let obj = agc_crcf_create();
            agc_crcf_set_bandwidth(obj, 0.25);
            agc_crcf_set_signal_level(obj, 1e-3);

            agc_crcf_squelch_enable(obj);
            agc_crcf_squelch_set_threshold(obj, -45.);
            agc_crcf_squelch_set_timeout(obj, 100);
            obj
        };

        Self { crcf }
    }
    pub fn execute(
        &mut self,
        signal: num_complex::Complex<f32>,
    ) -> (num_complex::Complex<f32>, SquelchStatus) {
        use liquid_dsp_bindings_sys::*;

        let mut value = __BindgenComplex {
            re: signal.re,
            im: signal.im,
        };

        unsafe { agc_crcf_execute(self.crcf as _, value, &mut value) };

        (num_complex::Complex::new(value.re, value.im), self.status())
    }

    pub fn status(&self) -> SquelchStatus {
        SquelchStatus::from_i32(unsafe {
            liquid_dsp_bindings_sys::agc_crcf_squelch_get_status(self.crcf)
        }).unwrap()
    }
}

impl Drop for CRCF {
    fn drop(&mut self) {
        unsafe { liquid_dsp_bindings_sys::agc_crcf_destroy(self.crcf) };
    }
}

struct Burst {
    crcf: CRCF,
    in_burst: bool,
    burst: Vec<num_complex::Complex<f32>>,
}

#[derive(FromPrimitive, Clone, Copy, Debug)]
pub enum SquelchStatus {
    Unknown = liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_UNKNOWN as _,
    Enabled = liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_ENABLED as _,
    Rise = liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_RISE as _,
    SignalHi = liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_SIGNALHI as _,
    Fall = liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_FALL as _,
    SignalLo = liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_SIGNALLO as _,
    Timeout = liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_TIMEOUT as _,
    Disabled = liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_DISABLED as _,
}

use chrono::prelude::*;

struct Packet {
    data: Vec<num_complex::Complex<f32>>,
    timestamp: DateTime<Utc>,
}

impl Burst {
    pub fn new() -> Self {
        Self {
            crcf: CRCF::new(),
            in_burst: false,
            burst: Vec::new(),
        }
    }

    pub fn catcher(&mut self, signal: num_complex::Complex<f32>) -> Option<Packet> {
        let (signal, status) = self.crcf.execute(signal);

        match status {
            SquelchStatus::Rise => {
                self.in_burst = true;
                self.burst.clear();
            }
            SquelchStatus::SignalHi => {
                self.burst.push(signal);
            }
            SquelchStatus::Timeout => {
                self.in_burst = false;
                let data = self.burst.clone();
                self.burst.clear();

                return Some(Packet {
                    data,
                    timestamp: Utc::now(),
                });
            }
            _ => {}
        }

        None
    }
}

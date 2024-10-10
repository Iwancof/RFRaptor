#![feature(iter_array_chunks)]
#![allow(internal_features)]
#![feature(core_intrinsics)]
#![feature(array_chunks)]
#![feature(let_chains)]

use core::fmt;

use anyhow::Context;
use soapysdr::Direction::Rx;

use core::mem::MaybeUninit;

use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

#[log_derive::logfn(ok = "TRACE", err = "ERROR")]
// #[tokio::main]
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
        center_freq: 2441.0e6, // bluetooth
        sample_rate: 20.0e6,
        bandwidth: 20.0e6,
        gain: 32.,
    };

    log::info!("config = {}", config);
    config.set(&dev)?;

    let mut magic: MaybeUninit<ice9_bindings::pfbch2_t> = MaybeUninit::uninit();
    let mut fsk: MaybeUninit<ice9_bindings::fsk_demod_t> = MaybeUninit::uninit();

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

        fsk_demod_init(fsk.as_mut_ptr());
    }

    let mut magic = unsafe { magic.assume_init() };
    println!("{:?}", magic);

    let mut fsk = unsafe { fsk.assume_init() };
    println!("{:?}", fsk);

    let mut stream = dev.rx_stream::<num_complex::Complex<i8>>(&[config.channels])?;
    // let mut _write_stream = dev.tx_stream::<num_complex::Complex<u8>>(&[config.channels])?;

    let sb = signalbool::SignalBool::new(&[signalbool::Signal::SIGINT], signalbool::Flag::Restart)?;

    let mut buffer = vec![num_complex::Complex::<i8>::new(0, 0); stream.mtu()?];
    let mut burst = (0..20)
        .into_iter()
        .map(|_| Burst::new())
        .collect::<Vec<_>>();

    stream.activate(None)?;

    const BATCH_SIZE: usize = 4096;

    // use std::io::{BufWriter, Write};
    // let mut file = BufWriter::new(std::fs::File::create("signal.dat")?);

    let mut planner = rustfft::FftPlanner::new();

    let mut fft_in_buffer = vec![Vec::with_capacity(BATCH_SIZE); 20];
    let mut output = vec![0i16; 96 * 2];

    create_catcher_threads();

    '_outer: for _ in 0.. {
        let read = stream.read(&mut [&mut buffer[..]], 1_000_000)?;
        assert_eq!(read, buffer.len());

        for chunk in buffer.chunks_mut(20 / 2) {
            unsafe {
                // SAFETY: Complex<T> has `repr(C)` layout
                let flat_chunk = chunk.as_mut_ptr() as *mut i8;

                ice9_bindings::pfbch2_execute(
                    &mut magic as _,
                    flat_chunk,
                    output.as_mut_ptr() as *mut i16,
                );
            }

            output[..20 * 2]
                .array_chunks::<2>()
                .enumerate()
                .for_each(|(i, [re, im])| {
                    let re = *re as f32 / 32768.0;
                    let im = *im as f32 / 32768.0;

                    let signal = num_complex::Complex::new(re, im);

                    fft_in_buffer[i].push(signal);
                });

            for (i, fft_in) in (&mut fft_in_buffer[..20]).into_iter().enumerate() {
                if fft_in.len() == BATCH_SIZE {
                    let fft = planner.plan_fft_inverse(4096);

                    fft.process(fft_in);

                    FFT_SIGNAL_CHANNEL[i]
                        .lock()
                        .unwrap()
                        .extend_from_slice(fft_in);

                    /*
                                        for e in &*fft_in {
                                            if let Some(mut packet) = burst[i].catcher(*e / 20 as f32)
                                                && 132 <= packet.data.len()
                                            {
                                                // log::info!("packet {}. timestamp: {}", packet.data.len(), packet.timestamp);

                                                unsafe {
                                                    use ice9_bindings::*;

                                                    let mut out = MaybeUninit::zeroed();
                                                    fsk_demod(
                                                        &mut fsk as _,
                                                        packet.data.as_mut_ptr() as _,
                                                        packet.data.len() as _,
                                                        out.as_mut_ptr(),
                                                    );

                                                    let out = out.assume_init();

                                                    if !out.demod.is_null() && !out.bits.is_null() {
                                                        // println!("found: {:?}", out);
                                                        let slice =
                                                            std::slice::from_raw_parts(out.bits, out.bits_len as usize);
                                                        println!(
                                                            "idx = {}, {} {} {:?}",
                                                            i,
                                                            packet.timestamp,
                                                            slice.len(),
                                                            &slice[..40]
                                                        );
                                                    }
                                                }
                                            }
                                        }
                    */
                }
            }
            for (i, fft_in) in (&mut fft_in_buffer).into_iter().enumerate() {
                if fft_in.len() == BATCH_SIZE {
                    fft_in.clear();
                }
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

#[derive(Debug)]
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
        })
        .unwrap()
    }
}

impl Drop for CRCF {
    fn drop(&mut self) {
        unsafe { liquid_dsp_bindings_sys::agc_crcf_destroy(self.crcf) };
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
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

    #[allow(unused)]
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

unsafe impl Send for Burst {}

static FFT_SIGNAL_CHANNEL: [std::sync::LazyLock<
    std::sync::Arc<std::sync::Mutex<Vec<num_complex::Complex<f32>>>>,
>; 20] = [const { std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))) };
    20];

fn create_catcher_threads() {
    for i in 0..20 {
        std::thread::spawn(move || {
            let mut burst = Burst::new();
            let mut fsk: MaybeUninit<ice9_bindings::fsk_demod_t> = MaybeUninit::uninit();

            unsafe {
                use ice9_bindings::*;

                fsk_demod_init(fsk.as_mut_ptr());
            }

            let mut fsk = unsafe { fsk.assume_init() };

            let mut tmp = Vec::with_capacity(4096);
            loop {
                core::mem::swap(&mut *FFT_SIGNAL_CHANNEL[i].lock().unwrap(), &mut tmp);

                if tmp.len() == 0 {
                    // tokio::time::sleep(tokio::time::Duration::from_millis(10));
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }

                for s in &tmp {
                    if let Some(mut packet) = burst.catcher(s / 20 as f32)
                        && 132 <= packet.data.len()
                    {
                        /*
                        log::info!(
                            "packet {}. timestamp: {}. idx: {}",
                            packet.data.len(),
                            packet.timestamp,
                            i
                        );
                        */

                        unsafe {
                            use ice9_bindings::*;

                            let mut out = MaybeUninit::zeroed();
                            fsk_demod(
                                &mut fsk as _,
                                packet.data.as_mut_ptr() as _,
                                packet.data.len() as _,
                                out.as_mut_ptr(),
                            );

                            let out = out.assume_init();

                            if !out.demod.is_null() && !out.bits.is_null() {
                                // println!("found: {:?}", out);
                                let slice: &mut [u8] =
                                    std::slice::from_raw_parts_mut(out.bits, out.bits_len as usize);
                                /*
                                                                println!(
                                                                    "idx = {}, {} {} {:?}",
                                                                    i,
                                                                    packet.timestamp,
                                                                    slice.len(),
                                                                    &slice[..40]
                                                                );
                                */

                                /*
                                                                if &slice[..6] == &[0, 1, 0, 1, 0, 1] {
                                                                    println!("found preamble");
                                                                }
                                */

                                use ice9_bindings::*;

                                let lap =
                                    btbb_find_ac(slice.as_mut_ptr() as _, slice.len() as _, 1);
                                // println!("lap = {:?}", lap);

                                if lap != 0xffffffff {
                                    let p = ble_easy(
                                        slice.as_mut_ptr() as _,
                                        slice.len() as _,
                                        (2441 + if i < 10 { i } else { i - 20 }) as _,
                                    );

                                    println!("p = {:?}", *p);
                                }
                            }
                        }
                    }
                }
                // tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                tmp.clear();
            }
        });
    }
}

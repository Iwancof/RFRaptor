#![feature(iter_array_chunks)]

use core::fmt;

use anyhow::Context;
use soapysdr::Direction::Rx;

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

    let mut stream = dev.rx_stream::<num_complex::Complex<f32>>(&[config.channels])?;
    let mut _write_stream = dev.tx_stream::<num_complex::Complex<f32>>(&[config.channels])?;

    let sb = signalbool::SignalBool::new(&[signalbool::Signal::SIGINT], signalbool::Flag::Restart)?;

    let mut buffer = vec![num_complex::Complex::new(0., 0.); stream.mtu()?];
    let mut burst = Burst::new();

    stream.activate(None)?;

    let mut ignore_count = 100;

    '_outer: for _ in 0.. {
        let read = stream.read(&mut [&mut buffer[..]], 1_000_000)?;

        assert_eq!(read, buffer.len());

        // FFT size is 1024
        const BATCH_SIZE: usize = 4096;
        let fft = rustfft::FftPlanner::new().plan_fft(BATCH_SIZE, rustfft::FftDirection::Inverse);

        for chunk in buffer.chunks_mut(BATCH_SIZE) {
            fft.process(chunk);

            println!("{:?}", &chunk[..10]);

            if ignore_count > 0 {
                ignore_count -= 1;
                continue;
            }

            let result = burst.catcher(chunk[512] / 20.);
            match result as _ {
                liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_SIGNALHI => {
                    // println!("signalhi");
                }
                liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_RISE => {
                    println!("rise");
                }
                liquid_dsp_bindings_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_TIMEOUT => {
                    println!("timeout");
                }
                x => {
                    println!("unknown {}", x);
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

struct Burst {
    crcf: liquid_dsp_bindings_sys::agc_crcf,
    burst: Vec<num_complex::Complex<f32>>,
}

impl Burst {
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

        Self {
            crcf,
            burst: Vec::new(),
        }
    }
    pub fn catcher(&mut self, signal: num_complex::Complex<f32>) -> i32 {
        use liquid_dsp_bindings_sys::*;
        let mut value = __BindgenComplex {
            re: signal.re,
            im: signal.im,
        };

        unsafe { agc_crcf_execute(self.crcf as _, value, &mut value) };

        unsafe { agc_crcf_squelch_get_status(self.crcf) }
    }
}

impl Drop for Burst {
    fn drop(&mut self) {
        unsafe { liquid_dsp_bindings_sys::agc_crcf_destroy(self.crcf) };
    }
}

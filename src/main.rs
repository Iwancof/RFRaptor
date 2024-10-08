#![feature(iter_array_chunks)]

use core::fmt;

use anyhow::Context;
use soapysdr::Direction::{Rx, Tx};

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

    stream.activate(None)?;

    use std::io::{Write, BufWriter};
    let mut file = BufWriter::new(std::fs::File::create("output.dat")?);

    'outer: for _ in 0..5 {
        let read = stream.read(&mut [&mut buffer[..]], 1_000_000)?;

        // FFT size is 1024
        const BATCH_SIZE: usize = 4096;
        let mut fft = rustfft::FftPlanner::new().plan_fft(BATCH_SIZE, rustfft::FftDirection::Inverse);

        for chunk in buffer.chunks_mut(BATCH_SIZE) {
            fft.process(chunk);

            // translate this python code `np.abs(np.fft.fft(x))**2 / (N*Fs)`
            let power = chunk.iter().map(|x| x.norm_sqr()).collect::<Vec<_>>();
            let power = power.iter().map(|x| x / (BATCH_SIZE as f32 * config.sample_rate as f32)).collect::<Vec<_>>();

            // make it log scale
            let power = power.iter().map(|x| 10.0 * x.log10()).collect::<Vec<_>>();

            let freq_step = config.sample_rate / BATCH_SIZE as f64;

            // shift the zero frequency to the center
            let mut power = power.iter().enumerate().map(|(i, x)| {
                let i = if i < BATCH_SIZE / 2 {
                    i as isize
                } else {
                    i as isize - BATCH_SIZE as isize
                };
                (i, x)
            }).collect::<Vec<_>>();

            // sort by frequency
            power.sort_by(|a, b| a.0.cmp(&b.0));

            // convert to frequency
            let power = power.iter().map(|(i, x)| (*i as f64 * freq_step, *x)).collect::<Vec<_>>();

            for p in power.iter() {
                writeln!(file, "{} {}", p.0, p.1)?;
            }

            break 'outer;
        }

        if sb.caught() {
            break;
        }
    }
    println!("read {} samples", buffer.len());

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

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

    // let stream = dev.rx_stream::<num_complex::Complex<f32>>(&[config.channels])?;
    let mut stream = dev.rx_stream::<num_complex::Complex<f32>>(&[config.channels])?;
    let sb = signalbool::SignalBool::new(&[signalbool::Signal::SIGINT], signalbool::Flag::Restart)?;

    let mut buffer = vec![num_complex::Complex::new(0., 0.); stream.mtu()?];

    let mut bits = Vec::new();
    let mut last_phase = 0.0;
    let mut last_bit = 0;

    stream.activate(None)?;
    for _ in 0..5 {
        let read = stream.read(&mut [&mut buffer[..]], 1_000_000)?;

        // FFT size is 1024
        let fft_size = 1024;
        let mut fft = rustfft::FftPlanner::new().plan_fft(fft_size, rustfft::FftDirection::Forward);

        buffer
            .windows(fft_size)
            .step_by(fft_size / 2)
            .for_each(|window| {
                let mut input: Vec<num_complex::Complex<f32>> = window.iter().map(|&x| x).collect();
                fft.process(&mut input);
                // println!("{:?}", input.len());
                // println!("{}", input[input.len() / 2].norm()); // 2426.0e6[Hz]の周波数成分の振幅を表示
                let signal = input[input.len() / 2];
                let phase = signal.arg();

                let mut diff = phase - last_phase;
                last_phase = phase;

                // println!("diff: {}", diff);
                if diff > std::f32::consts::PI {
                    diff -= 2.0 * std::f32::consts::PI;
                } else if diff < -std::f32::consts::PI {
                    diff += 2.0 * std::f32::consts::PI;
                }

                /*
                                let bit = if diff > 0.0 {
                                    1 - last_bit
                                } else {
                                    last_bit
                                };
                */
                let bit = if diff > 0.0 { 1 } else { 0 };

                bits.push(bit);
                last_bit = bit;
            });

        // println!();

        // break; // for temporary testing

        if sb.caught() {
            break;
        }
    }

    for offset in 0..8 {
        println!("offset: {}", offset);
        let bytes: Vec<u8> = bits
            .iter()
            .skip(offset)
            .array_chunks::<8>()
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0, |acc, (i, &bit)| acc + (bit << i))
            })
            .collect();

        // dump as hex
        for byte in bytes {
            print!("{:02x} ", byte);
        }
        println!();
    }

    println!("read {} samples", buffer.len());
    // for example,

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

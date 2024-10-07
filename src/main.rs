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

    for _ in 0..5 {
        let read = stream.read(&mut [&mut buffer[..]], 1_000_000)?;

        // FFT size is 1024
        // let fft_size = 1024;
        // let mut fft = rustfft::FftPlanner::new().plan_fft(fft_size, rustfft::FftDirection::Forward);

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

enum BufferState {
    Empty,
    InUse,
    Ready,
}

struct WindowBuffer<'a, T: soapysdr::StreamSample> {
    stream: &'a mut soapysdr::RxStream<T>,
    internal_buffer: [Arc<Mutex<Vec<T>>>; 2],
    buffer_states: Arc<Mutex<[BufferState; 2]>>,
    windows_size: usize,
}

impl<'a, T: soapysdr::StreamSample> WindowBuffer<'a, T> {
    const INTERNAL_BUFFER_SIZE: usize = 1024 * 128;

    fn new(stream: &'a mut soapysdr::RxStream<T>, windows_size: usize) -> Self
    where
        T: Default + Clone, // FIXME
    {
        // let internal_buffer = vec![T::default(); Self::INTERNAL_BUFFER_SIZE];
        let internal_buffer = [
            Arc::new(Mutex::new(vec![T::default(); Self::INTERNAL_BUFFER_SIZE])),
            Arc::new(Mutex::new(vec![T::default(); Self::INTERNAL_BUFFER_SIZE])),
        ];

        let buffer_states = Arc::new(Mutex::new([BufferState::Empty, BufferState::Empty]));

        Self {
            stream,
            internal_buffer,
            buffer_states,
            windows_size,
        }
    }

    fn start_read_thread(&self) -> anyhow::Result<()> {
        let buffer_states = self.buffer_states.clone();
    }
}

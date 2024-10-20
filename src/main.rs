#![feature(iter_array_chunks)]
#![allow(internal_features)]
#![feature(core_intrinsics)]
#![feature(array_chunks)]
#![feature(let_chains)]

mod burst;
mod channelizer;

use burst::Burst;

use core::fmt;

use anyhow::Context;
use soapysdr::Direction::Rx;

use core::mem::MaybeUninit;

use num_complex::Complex;

#[log_derive::logfn(ok = "TRACE", err = "ERROR")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
        center_freq: 2427.0e6, // bluetooth
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

    let mut stream = dev.rx_stream::<Complex<i8>>(&[config.channels])?;

    let sb = signalbool::SignalBool::new(&[signalbool::Signal::SIGINT], signalbool::Flag::Restart)?;

    const BATCH_SIZE: usize = 4096;

    let mut planner = rustfft::FftPlanner::new();

    let mut fft_in_buffer = Vec::with_capacity(BATCH_SIZE * 20);

    let fft = planner.plan_fft_inverse(20);

    create_catcher_threads();


    // fixed size buffer
    let mut buffer = vec![Complex::<i8>::new(0, 0); stream.mtu()?].into_boxed_slice(); 

    stream.activate(None)?;
    '_outer: for _ in 0.. {
        let read = stream.read(&mut [&mut buffer[..]], 1_000_000)?;
        assert_eq!(read, buffer.len());

        for chunk in buffer.chunks_mut(20 / 2) {
            if chunk.len() != 20 / 2 {
                continue;
            }

            let output = channelizer::channelize(&mut magic, chunk);
            fft_in_buffer.extend_from_slice(&output);

            if fft_in_buffer.len() == BATCH_SIZE * 20 {
                let mut fft_out_buffer = vec![Vec::with_capacity(4096); 20];

                for (_batch_idx, fft_in) in fft_in_buffer.chunks_mut(20).enumerate() {
                    fft.process(fft_in);

                    for (i, fft_in) in fft_in.iter().enumerate() {
                        fft_out_buffer[i].push(*fft_in);
                    }
                }

                assert_eq!(fft_out_buffer.len(), 20);
                assert_eq!(fft_out_buffer[0].len(), 4096);

                for (channel_idx, fft_out) in fft_out_buffer.iter_mut().enumerate() {
                    let mut append_target = FFT_SIGNAL_CHANNEL[channel_idx].lock().unwrap();
                    if 4096 * 128 <= append_target.len() {
                        // ignore
                    } else {
                        append_target.extend_from_slice(fft_out);
                    }
                }

                fft_in_buffer.clear();
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

static FFT_SIGNAL_CHANNEL: [std::sync::LazyLock<
    std::sync::Arc<std::sync::Mutex<Vec<Complex<f32>>>>,
>; 20] = [const { std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))) };
    20];

fn create_catcher_threads() {
    let mut first_live = usize::MAX;
    let mut last_live = usize::MIN;

    let mut ble_ch_to_sdr_idx = [None; 96];

    for i in 0..20 {
        let freq = (2427 as isize + if i < 10 { i } else { -20 + i }) as usize;
        if freq & 1 == 0 && freq >= 2402 && freq <= 2480 {
            let ch_num = (freq - 2402) / 2;
            if ch_num < first_live {
                first_live = ch_num;
            }
            if ch_num > last_live {
                last_live = ch_num;
            }

            ble_ch_to_sdr_idx[ch_num] = Some(i as usize);
        }
    }

    for ble_ch_idx in first_live..=last_live {
        let freq = 2402 + 2 * ble_ch_idx as u32;
        tokio::spawn(async move {
            // std::thread::spawn(move || {
            let mut burst = Burst::new();
            let mut fsk: MaybeUninit<ice9_bindings::fsk_demod_t> = MaybeUninit::uninit();

            unsafe {
                use ice9_bindings::*;

                fsk_demod_init(fsk.as_mut_ptr());
            }

            let mut fsk = unsafe { fsk.assume_init() };

            let mut tmp = Vec::with_capacity(4096);
            loop {
                core::mem::swap(
                    &mut *FFT_SIGNAL_CHANNEL[ble_ch_to_sdr_idx[ble_ch_idx].unwrap()]
                        .lock()
                        .unwrap(),
                    &mut tmp,
                );

                if tmp.len() == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }

                for agc_array in tmp.chunks(4096) {
                    for s in agc_array {
                        if let Some(packet) = burst.catcher(s / 20 as f32) {
                            // println!("id = {i}, length: {}", packet.data.len());

                            if packet.data.len() < 132 {
                                continue;
                            }

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
                                    let slice: &mut [u8] = std::slice::from_raw_parts_mut(
                                        out.bits,
                                        out.bits_len as usize,
                                    );

                                    use ice9_bindings::*;

                                    let lap =
                                        btbb_find_ac(slice.as_mut_ptr() as _, slice.len() as _, 1);

                                    if lap == 0xffffffff {
                                        let p = ble_easy(
                                            slice.as_mut_ptr() as _,
                                            slice.len() as _,
                                            freq,
                                        );

                                        if !p.is_null() {
                                            let len = (*p).len as usize;
                                            let slice = (*p).data.as_slice(len);

                                            let flag = slice[4] & 0b1111;
                                            if flag == 0 || flag == 2 {
                                                let mut mac = slice[6..(6 + 6)].to_vec();
                                                mac.reverse();

                                                println!(
                                                    "mac = {:2x?}, freq = {}, data = {:x?}",
                                                    mac, freq, slice
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                tmp.clear();
            }
        });
    }
}

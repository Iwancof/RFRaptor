#![feature(iter_array_chunks)]
#![allow(internal_features)]
#![feature(core_intrinsics)]
#![feature(array_chunks)]
#![feature(let_chains)]

mod bluetooth;
mod burst;
mod channelizer;
mod fsk;
mod sdr;

use burst::Burst;
use fsk::FskDemod;
use sdr::SDRConfig;

use anyhow::Context;
use core::mem::MaybeUninit;

use num_complex::Complex;

use tungstenite::accept;

// Config at runtime
static SDR_CONFIG: std::sync::LazyLock<std::sync::Arc<std::sync::Mutex<Option<SDRConfig>>>> =
    const { std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(None))) };

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
        center_freq: 2427.0e6, // bluetooth
        sample_rate: 20.0e6,
        bandwidth: 20.0e6,
        gain: 32.,
    };
    SDR_CONFIG.lock().unwrap().replace(config.clone());

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

    let mut stream = dev.rx_stream::<Complex<i8>>(&[config.channels])?;

    let sb = signalbool::SignalBool::new(&[signalbool::Signal::SIGINT], signalbool::Flag::Restart)?;

    const BATCH_SIZE: usize = 4096;

    let mut planner = rustfft::FftPlanner::new();

    let mut fft_in_buffer = Vec::with_capacity(BATCH_SIZE * 20);
    let fft = planner.plan_fft_inverse(20);

    let mut is_buffer_valid = [false; 96];
    for i in 0..20 {
        let freq = (2427 as isize + if i < 10 { i } else { -20 + i }) as usize;
        if freq & 1 == 0 && freq >= 2402 && freq <= 2480 {
            is_buffer_valid[i as usize] = true;
        }
    }

    // fixed size buffer
    let mut buffer = vec![Complex::<i8>::new(0, 0); stream.mtu()?].into_boxed_slice();

    create_catcher_threads();
    start_websocket()?;

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

                for fft_working in fft_in_buffer.chunks_mut(20) {
                    fft.process(fft_working);

                    for (i, fft_in) in fft_working.iter().enumerate() {
                        fft_out_buffer[i].push(*fft_in);
                    }
                }

                assert_eq!(fft_out_buffer.len(), 20);
                assert_eq!(fft_out_buffer[0].len(), 4096);

                for (channel_idx, fft_out) in fft_out_buffer.iter_mut().enumerate() {
                    if !is_buffer_valid[channel_idx] {
                        continue;
                    }
                    FFT_SIGNAL_CHANNEL[channel_idx]
                        .lock()
                        .unwrap()
                        .extend_from_slice(fft_out);
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

static FFT_SIGNAL_CHANNEL: [std::sync::LazyLock<
    std::sync::Arc<std::sync::Mutex<Vec<Complex<f32>>>>,
>; 96] = [const { std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))) };
    96];

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
        std::thread::spawn(move || {
            let mut burst = Burst::new();
            let mut fsk = FskDemod::new();

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

                for s in &tmp {
                    if let Some(packet) = burst.catcher(s / 20 as f32) {
                        if packet.data.len() < 132 {
                            continue;
                        }
                        if let Some(out) = fsk.demod(&packet.data) {
                            if let Ok(bt) = bluetooth::Bluetooth::from_packet(&out, freq) {
                                if let bluetooth::BluetoothPacket::Advertisement(ref adv) =
                                    bt.packet
                                {
                                    // println!("{}. remain: {:x?}", adv, bt.remain);
                                    log::info!("{}. remain: {:x?}", adv, bt.remain);
                                }

                                PACKETS.lock().unwrap().push_back(bt);
                            }
                        }
                    }
                }

                tmp.clear();
            }
        });
    }
}

static PACKETS: std::sync::LazyLock<
    std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<bluetooth::Bluetooth>>>,
> = const {
    std::sync::LazyLock::new(|| {
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()))
    })
};

fn start_websocket() -> anyhow::Result<()> {
    let server = std::net::TcpListener::bind("127.0.0.1:8080")?;

    std::thread::spawn(move || {
        for stream in server.incoming() {
            let stream = stream.unwrap();
            std::thread::spawn(move || {
                let mut ws = accept(stream).unwrap();

                loop {
                    let bt = PACKETS.lock().unwrap().pop_front();

                    if let Some(bt) = bt {
                        #[allow(non_snake_case)]
                        #[derive(serde_derive::Serialize)]
                        struct Message {
                            mac: String,
                            packetInfo: String,
                            packetBytes: String,
                        }

                        if let bluetooth::BluetoothPacket::Advertisement(ref adv) = bt.packet {
                            let msg = Message {
                                mac: format!("{}", adv.address),
                                packetInfo: format!("{}", adv),
                                packetBytes: format!("{:x?}", bt.bytes),
                            };

                            ws.send(tungstenite::Message::Text(
                                serde_json::to_string(&msg).unwrap(),
                            ))
                            .unwrap();
                        }
                    }
                }
            });
        }
    });

    Ok(())
}

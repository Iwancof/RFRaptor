#![feature(array_chunks)]
#![feature(portable_simd)]
#![feature(test)]
#![feature(try_blocks)]
#![feature(generic_arg_infer)]

mod bitops;
mod bluetooth;
mod burst;
mod channelizer;
mod fsk;
mod sdr;

use burst::Burst;
use fsk::FskDemod;
use sdr::SDRConfig;

use anyhow::Context;

use num_complex::Complex;

use tungstenite::accept;

const NUM_CHANNELS: usize = 20usize;

type ChannelReceiver = (usize, std::sync::mpsc::Receiver<Vec<Complex<f32>>>);

// Config at runtime
static SDR_CONFIG: std::sync::LazyLock<std::sync::Arc<std::sync::Mutex<Option<SDRConfig>>>> =
    const { std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(None))) };

#[log_derive::logfn(ok = "TRACE", err = "ERROR")]
fn main() -> anyhow::Result<()> {
    std::env::set_var(
        "SOAPY_SDR_PLUGIN_PATH",
        "/home/iwancof/Nextcloud/SecHack365/HackRF/soapy-virtual/build",
    );

    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    soapysdr::configure_logging();

    #[link(name = "fftw3f_threads")]
    extern "C" {
        fn fftwf_make_planner_thread_safe();
    }

    unsafe {
        fftwf_make_planner_thread_safe()
    }

    // let filter = "virtual";
    let filter = "hackrf";
    log::trace!("filter is {}", filter);

    let devarg = soapysdr::enumerate(filter)
        .context("failed to enumerate devices")?
        .into_iter()
        .next()
        .context("No devices found")?;
    log::trace!("found device {}", devarg);

    let dev = soapysdr::Device::new(devarg)?;

    // let center_freq = 2480;
    // let center_freq = 2401;
    let center_freq = 2427;

    let m = 4;
    let lp_cutoff = 0.75;

    let config = SDRConfig {
        channels: 0,
        num_channels: NUM_CHANNELS,
        center_freq: center_freq as f64 * 1.0e6,
        sample_rate: 20.0e6,
        bandwidth: 20.0e6,
        gain: 64.,
    };
    SDR_CONFIG.lock().unwrap().replace(config.clone());

    log::info!("config = {}", config);
    config.set(&dev)?;

    let mut channelizer = channelizer::Channelizer::new(NUM_CHANNELS, m, lp_cutoff);

    let mut read_stream = dev.rx_stream::<Complex<i8>>(&[config.channels])?;

    let sb = signalbool::SignalBool::new(&[signalbool::Signal::SIGINT], signalbool::Flag::Restart)?;

    // fixed size buffer
    let mut buffer = vec![Complex::<i8>::new(0, 0); read_stream.mtu()?].into_boxed_slice();

    // let mut is_buffer_valid = [false; 96];
    let mut sdridx_to_sender = vec![];
    let mut blch_to_receiver = vec![];

    for _ in 0..NUM_CHANNELS {
        sdridx_to_sender.push(None);
    }
    for _ in 0..96 {
        blch_to_receiver.push(None);
    }

    for (sdr_idx, (tx, rx)) in (0..NUM_CHANNELS)
        .map(|_| std::sync::mpsc::channel::<Vec<Complex<f32>>>())
        .enumerate()
    {
        let sdr_idx_isize = sdr_idx as isize;
        let freq = center_freq
            + if sdr_idx_isize < (NUM_CHANNELS as isize / 2) {
                sdr_idx_isize
            } else {
                sdr_idx_isize - NUM_CHANNELS as isize
            };

        if freq & 1 == 0 && (2402..=2480).contains(&freq) {
            let blch = ((freq - 2402) / 2) as usize;

            sdridx_to_sender[sdr_idx] = Some((blch, tx));
            blch_to_receiver[blch] = Some((sdr_idx, rx));
        }
    }

    create_catcher_threads(blch_to_receiver);
    start_websocket()?;

    let mut fft_result: Vec<Vec<Complex<f32>>> = (0..NUM_CHANNELS)
        .map(|_| Vec::with_capacity(131072 / (NUM_CHANNELS / 2)))
        .collect::<Vec<_>>();

    read_stream.activate(None)?;
    '_outer: for _ in 0.. {
        let _read = read_stream.read(&mut [&mut buffer[..]], 1_000_000)?;
        // assert_eq!(read, buffer.len());

        for fft in fft_result.iter_mut() {
            fft.clear();
        }

        for chunk in buffer.chunks_exact_mut(NUM_CHANNELS / 2) {
            for (sdridx, fft) in channelizer.channelize_fft(chunk).iter().enumerate() {
                if sdridx_to_sender[sdridx].is_some() {
                    // fft_result[sdridx].push(*fft / (NUM_CHANNELS) as f32);
                    fft_result[sdridx].push(*fft);
                }
            }
        }

        for ch_idx in 0..NUM_CHANNELS {
            if let Some((_blch, tx)) = &sdridx_to_sender[ch_idx] {
                tx.send(fft_result[ch_idx].clone())?;
            }
        }

        if sb.caught() {
            break;
        }
    }

    read_stream.deactivate(None)?;

    Ok(())
}

fn create_catcher_threads(rxs: Vec<Option<ChannelReceiver>>) {
    let sample_rate = SDR_CONFIG.lock().unwrap().as_ref().unwrap().sample_rate as f32;
    let num_channels = SDR_CONFIG.lock().unwrap().as_ref().unwrap().num_channels;

    for (ble_ch_idx, sdr_idx_rx) in rxs
        .into_iter()
        .enumerate()
        .filter(|(_, sdr_idx_rx)| sdr_idx_rx.is_some())
    {
        let freq = 2402 + 2 * ble_ch_idx as u32;

        let (_sdr_idx, rx) = sdr_idx_rx.unwrap();
        std::thread::spawn(move || {
            let mut burst = Burst::new();
            let mut fsk = FskDemod::new(sample_rate, num_channels);

            #[derive(Debug)]
            enum ErrorKind {
                Catcher,
                Demod,
                Bitops,
                Bluetooth,
            }

            loop {
                let Ok(received) = rx.recv() else {
                    break;
                };

                for s in received {
                    let ret: Result<(), ErrorKind> = try {
                        let packet = burst
                            .catcher(s / num_channels as f32)
                            .ok_or(ErrorKind::Catcher)?;

                        if packet.data.len() < 132 {
                            continue;
                        }

                        let demodulated = fsk.demodulate(packet.data).ok_or(ErrorKind::Demod)?;

                        let (remain_bits, byte_packet) =
                            bitops::bits_to_packet(&demodulated, freq as usize)
                                .map_err(|_| ErrorKind::Bitops)?;

                        if !remain_bits.is_empty() {
                            log::trace!("remain bits: {:?}", remain_bits);
                        }

                        let bt = bluetooth::Bluetooth::from_bytes(byte_packet, freq as usize)
                            .map_err(|_| ErrorKind::Bluetooth)?;

                        PACKETS.lock().unwrap().push_back(bt.clone());
                        if let bluetooth::PacketInner::Advertisement(ref adv) = bt.packet.inner {
                            // log::info!("{}. remain: {}", adv, byte_to_ascii_string(&bt.remain));
                            log::info!("{}", adv);

                            // let cfg = pretty_hex::HexConfig { title: false, width: 8, group: 0, ..Default::default() };
                            // let hex = pretty_hex::config_hex(&bt.remain, cfg);
                            // log::info!("\n{}", hex);
                        }
                    };

                    let Err(kind) = ret else {
                        continue;
                    };

                    match kind {
                        ErrorKind::Catcher => {
                            //
                        }
                        ErrorKind::Demod => {
                            //
                        }
                        ErrorKind::Bitops => {
                            //
                        }
                        ErrorKind::Bluetooth => {
                            log::error!("failed to bluetooth");
                        }
                    }
                }
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

#[allow(dead_code)]
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

                        if let bluetooth::PacketInner::Advertisement(ref adv) = bt.packet.inner {
                            let msg = Message {
                                mac: format!("{}", adv.address),
                                packetInfo: format!("{}", adv),
                                packetBytes: format!("{:x?}", bt.bytes_packet),
                            };

                            println!("sent");

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

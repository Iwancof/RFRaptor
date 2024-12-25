#![feature(array_chunks)]
#![feature(portable_simd)]
#![feature(test)]
#![feature(try_blocks)]
#![feature(generic_arg_infer)]

mod bitops;
mod bluetooth;
mod burst;
mod channelizer;
mod device;
mod fsk;
mod liquid;

use burst::Burst;
use fsk::FskDemod;

use num_complex::Complex;
use soapysdr::Device;
use tungstenite::accept;

use clap::Parser;

#[allow(unused_imports)] // use with permission
use thread_priority::{set_current_thread_priority, ThreadPriority};

type ChannelReceiver = (usize, std::sync::mpsc::Receiver<Vec<Complex<f32>>>);
type ChannelSender = (usize, std::sync::mpsc::Sender<Vec<Complex<f32>>>);

use device::sdr::SDRConfig;
use device::{SDR_RX_CONFIGS, SDR_TX_CONFIGS};

static RUNNING: std::sync::LazyLock<std::sync::Arc<std::sync::atomic::AtomicBool>> = const {
    std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)))
};

#[derive(Parser, Debug)]
#[command(
    name = format!("hydro-strike CLI Tool v{} hash={}", env!("CARGO_PKG_VERSION"), env!("GIT_HASH")),
    version = format!("{}-{}", env!("CARGO_PKG_VERSION"), env!("GIT_HASH")),
    about = "Welcome to hydro-strike CLI Tool",
)]
pub(crate) struct Args {
    #[arg(short, long)]
    path: String,
}

#[log_derive::logfn(ok = "TRACE", err = "ERROR")]
fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    soapysdr::configure_logging();

    let args = Args::parse();

    let (rx, tx) = device::open_device(args.path)?;
    println!("rx.len() = {}, tx.len() = {}", rx.len(), tx.len());

    println!("configs: {:?}", SDR_RX_CONFIGS.lock().unwrap());
    println!("configs: {:?}", SDR_TX_CONFIGS.lock().unwrap());

    // NOTE: use first device for testing
    let read_dev = rx[0].clone();
    let write_dev = tx[0].clone();

    let read_config = SDR_RX_CONFIGS.lock().unwrap()[&0].clone();

    let mut write_stream =
        write_dev.tx_stream_args::<Complex<i8>, _>(&[read_config.channels], "")?;

    write_stream.activate(None)?;
    write_stream.write(&[&[Complex::new(0, 0); 1024]], None, true, 1_000_000)?;
    write_stream.deactivate(None)?;

    // let mut is_buffer_valid = [false; 96];
    let mut sdridx_to_sender: Vec<Option<ChannelSender>> = vec![];
    let mut blch_to_receiver: Vec<Option<ChannelReceiver>> = vec![];

    for _ in 0..read_config.num_channels {
        sdridx_to_sender.push(None);
    }
    for _ in 0..96 {
        blch_to_receiver.push(None);
    }

    for (sdr_idx, (tx, rx)) in (0..read_config.num_channels)
        .map(|_| std::sync::mpsc::channel::<Vec<Complex<f32>>>())
        .enumerate()
    {
        let sdr_idx_isize = sdr_idx as isize;
        let freq = read_config.freq_mhz as isize
            + if sdr_idx_isize < (read_config.num_channels as isize / 2) {
                sdr_idx_isize
            } else {
                sdr_idx_isize - read_config.num_channels as isize
            };

        if freq & 1 == 0 && (2402..=2480).contains(&freq) {
            let blch = ((freq - 2402) / 2) as usize;

            sdridx_to_sender[sdr_idx] = Some((blch, tx));
            blch_to_receiver[blch] = Some((sdr_idx, rx));
        }
    }

    create_catcher_threads(blch_to_receiver, read_config.clone());
    start_websocket()?;

    start_rx_handler(read_dev, read_config, sdridx_to_sender);

    let sb = signalbool::SignalBool::new(&[signalbool::Signal::SIGINT], signalbool::Flag::Restart)?;
    loop {
        if sb.caught() || !RUNNING.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
    }

    RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);

    Ok(())
}

fn start_rx_handler(
    device: Device,
    config: SDRConfig,
    sdridx_to_sender: Vec<Option<ChannelSender>>,
) {
    std::thread::spawn(move || {
        let ret: anyhow::Result<()> = try {
            let mut channelizer = channelizer::Channelizer::new(config.num_channels, 4, 0.75);

            let mut read_stream =
                device.rx_stream_args::<Complex<i8>, _>(&[config.channels], "buffers=65535")?;

            let mut fft_result: Vec<Vec<Complex<f32>>> = (0..config.num_channels)
                .map(|_| Vec::with_capacity(131072 / (config.num_channels / 2)))
                .collect::<Vec<_>>();

            // fixed size buffer
            let mut buffer = vec![Complex::new(0, 0); read_stream.mtu()?].into_boxed_slice();

            println!("read_config: {}", config);

            read_stream.activate(None)?;
            '_outer: for _ in 0.. {
                let _read = read_stream.read(&mut [&mut buffer[..]], 1_000_000)?;
                // println!("read: {}", _read);
                // println!("{:?}", &buffer[_read-3..]);
                // assert_eq!(read, buffer.len());

                if let Some(remain_count) = device
                    .channel_info(soapysdr::Direction::Rx, 0)?
                    .get("buffer_count")
                {
                    let remain_count = remain_count.parse::<usize>()?;
                    log::trace!("remain_count: {}", remain_count);

                    // if 1000 < remain_count {
                    //     log::warn!("processing too slow: {}", remain_count);
                    // }
                }

                for fft in fft_result.iter_mut() {
                    fft.clear();
                }

                for chunk in buffer.chunks_exact_mut(config.num_channels / 2) {
                    for (sdridx, fft) in channelizer.channelize_fft(chunk).iter().enumerate() {
                        if sdridx_to_sender[sdridx].is_some() {
                            fft_result[sdridx].push(*fft);
                        }
                    }
                }

                for ch_idx in 0..config.num_channels {
                    if let Some((_blch, tx)) = &sdridx_to_sender[ch_idx] {
                        tx.send(fft_result[ch_idx].clone())?;
                    }
                }

                if !RUNNING.load(std::sync::atomic::Ordering::Relaxed) {
                    break '_outer;
                }
            }

            read_stream.deactivate(None)?;
        };

        if let Err(e) = ret {
            log::error!("failed to start_rx_handler: {}", e);
            RUNNING.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    });
}

fn create_catcher_threads(rxs: Vec<Option<ChannelReceiver>>, config: SDRConfig) {
    let sample_rate = config.sample_rate;
    let num_channels = config.num_channels;

    for (ble_ch_idx, sdr_idx_rx) in rxs
        .into_iter()
        .enumerate()
        .filter(|(_, sdr_idx_rx)| sdr_idx_rx.is_some())
    {
        let freq = 2402 + 2 * ble_ch_idx as u32;

        let (_sdr_idx, rx) = sdr_idx_rx.unwrap();
        std::thread::spawn(move || {
            let mut burst = Burst::new();
            let mut fsk = FskDemod::new(sample_rate as _, num_channels);

            #[derive(Debug)]
            enum ErrorKind {
                Catcher,
                Demod(anyhow::Error),
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

                        let demodulated = fsk
                            .demodulate(packet.data)
                            .map_err(|e| ErrorKind::Demod(e))?;

                        let (remain_bits, byte_packet) =
                            bitops::bits_to_packet(&demodulated.bits, freq as usize)
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
                        ErrorKind::Demod(d) => {
                            // log::error!("failed to demodulate: {}", d);
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
                        #[derive(serde::Serialize)]
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

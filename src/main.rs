use hydro_strike::*;

use clap::Parser;

use anyhow::Context;

use stream::ProcessFailKind;
#[allow(unused_imports)] // use with permission use thread_priority::{set_current_thread_priority, ThreadPriority};
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

    let file = std::fs::File::open(args.path)?;

    let config: device::config::List =
        serde_yaml::from_reader(file).context("failed to parse config")?;

    let mut streams = device::open_device(config)?;
    println!("streams: {:?}", streams.len());

    let mut stop_signals = vec![];
    for s in &streams {
        stop_signals.push(s.running.clone());
    }

    ctrlc::set_handler(move || {
        log::warn!("ctrl-c received, stopping...");
        for s in &stop_signals {
            *s.lock().unwrap() = false;
        }
    })?;

    if streams.len() == 1 {
        #[allow(unused_mut)]
        let mut hackrf_rx = streams.remove(0);
        println!("hackrf_rx: {:?}", hackrf_rx.config);

        let mut demod_counter = 0;
        for r in hackrf_rx.start_rx_with_error()? {
            use stream::StreamResult;

            match r {
                StreamResult::Packet(p) => {
                    // log::info!("Packet: {:x?}", p.packet);
                    // log::info!("freq: {}", p.bytes_packet.freq);
                    // log::info!("{:x?}", p.bytes_packet.bytes);

                    if let crate::bluetooth::PacketInner::Advertisement(ref adv) = p.packet.inner {
                        if adv.address
                            == (bluetooth::MacAddress {
                                // 18:09:d4:00:81:fb
                                address: [0xfb, 0x81, 0x00, 0xd4, 0x09, 0x18],
                            })
                        {
                            log::info!(
                                "rssi = {}",
                                p.bytes_packet.raw.unwrap().raw.unwrap().rssi_average
                            );
                            log::info!("{}", adv);
                        }
                    }
                }
                StreamResult::Error(e) => {
                    log::error!("Error: {}", e);
                    break;
                }
                StreamResult::ProcessFail(ProcessFailKind::Demod(_)) => {
                    demod_counter += 1;
                }
                StreamResult::ProcessFail(_kind) => {}
            }
        }

        println!("done, demod_counter = {}", demod_counter);
        *hackrf_rx.running.lock().unwrap() = false;
    } else {
        #[allow(unused_mut)]
        let mut sample_rx = streams.remove(0);
        #[allow(unused_mut)]
        let mut hackrf_rx = streams.remove(0);
        #[allow(unused_mut)]
        let mut hackrf_tx = streams.remove(0);

        println!("sample_rx: {:?}", sample_rx.config);
        println!("hackrf_rx: {:?}", hackrf_rx.config);
        println!("hackrf_tx: {:?}", hackrf_tx.config);

        let _handle = std::thread::spawn(move || {
            // wait reader
            std::thread::sleep(std::time::Duration::from_secs(1));
            log::warn!("start tx");

            *sample_rx.running.lock().unwrap() = true;
            *hackrf_tx.running.lock().unwrap() = true;

            // *tx[0].running.lock().unwrap() = true;
            // let mut stream = tx[0].raw.tx_stream(&[0]).unwrap();

            // // tx[0].raw.tx_stream(

            // let mut syn = channelizer::Synthesizer::new(16);
            // let mut modulater = fsk::FskMod::new(20e6, 16);
            // let bytes = (0..0x80).map(|i| i as u8).collect::<Vec<_>>();

            // let bits = bitops::packet_to_bits(&bytes, 2426, 0xdeadbeef);
            // let modulated = modulater.modulate(&bits).unwrap();

            // let mut synthesized = vec![];
            // for &s in &modulated {
            //     let mut signals = vec![num_complex::Complex32::new(0., 0.); 16];
            //     signals[8] = s;

            //     let s = syn.synthesize(&signals);
            //     synthesized.extend_from_slice(&s);
            // }

            // read from sample
            let mut rx_stream = sample_rx.raw.rx_stream(&[0]).unwrap();
            let mut tx_stream = hackrf_tx.raw.tx_stream(&[0]).unwrap();

            rx_stream.activate(None).unwrap();
            tx_stream.activate(None).unwrap();

            let mut total = vec![];

            loop {
                let mut buffer = vec![num_complex::Complex32::default(); rx_stream.mtu().unwrap()];
                let _r = match rx_stream.read(&mut [&mut buffer], 1_000_000) {
                    Ok(r) => r,
                    Err(_) => {
                        break;
                    }
                };

                total.extend_from_slice(&buffer);

                if !*sample_rx.running.lock().unwrap() {
                    break;
                }
                if !*hackrf_tx.running.lock().unwrap() {
                    break;
                }
            }

            tx_stream
                .write_all(&[&total], None, true, 1_000_000_000)
                .unwrap();

            tx_stream.deactivate(None).unwrap();
            rx_stream.deactivate(None).unwrap();

            *sample_rx.running.lock().unwrap() = false;
            *hackrf_tx.running.lock().unwrap() = false;

            log::warn!("tx done");
        });

        let mut demod_counter = 0;
        for r in hackrf_rx.start_rx_with_error()? {
            use stream::StreamResult;

            let finding_mac = [bluetooth::MacAddress {
                // 4b:95:2b:3c:95:bf
                address: [0xbf, 0x95, 0x3c, 0x2b, 0x95, 0x4b],
            }];

            match r {
                StreamResult::Packet(p) => {
                    if let crate::bluetooth::PacketInner::Advertisement(ref adv) = p.packet.inner {
                        let mac = &adv.address;

                        if finding_mac.contains(mac) {
                            log::info!(
                                "rssi = {}",
                                p.bytes_packet.raw.unwrap().raw.unwrap().rssi_average
                            );
                            log::info!("{}", adv);
                        }
                    }
                }
                StreamResult::Error(e) => {
                    if e.to_string().contains("Interrupted") {
                        break;
                    }
                }
                StreamResult::ProcessFail(ProcessFailKind::Demod(_)) => {
                    demod_counter += 1;
                }
                StreamResult::ProcessFail(_kind) => {}
            }
        }

        println!("done, demod_counter = {}", demod_counter);
        *hackrf_rx.running.lock().unwrap() = false;
    }

    Ok(())
}

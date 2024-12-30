use hydro_strike::*;

use clap::Parser;

use anyhow::Context;

use stream::ProcessFailKind;
#[allow(unused_imports)] // use with permission
use thread_priority::{set_current_thread_priority, ThreadPriority};

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

    let (mut rx, tx) = device::open_device(config)?;
    println!("rx.len() = {}, tx.len() = {}", rx.len(), tx.len());

    let stop_signal = rx[0].running.clone();
    ctrlc::set_handler(move || {
        *stop_signal.lock().unwrap() = false;
    })?;

    /*
    for packet in rx[0].start_rx()? {
        if let crate::bluetooth::PacketInner::Advertisement(ref adv) = packet.packet.inner {
            log::info!(
                "{}",
                packet.bytes_packet.raw.unwrap().raw.unwrap().rssi_average
            );
            log::info!("{}", adv);
        }
    }
    */

    let mut demod_counter = 0;
    for r in rx[0].start_rx_with_error()? {
        use stream::StreamResult;

        match r {
            StreamResult::Packet(p) => {
                if let crate::bluetooth::PacketInner::Advertisement(ref adv) = p.packet.inner {
                    log::info!("{}", p.bytes_packet.raw.unwrap().raw.unwrap().rssi_average);
                    log::info!("{}", adv);
                }
            }
            StreamResult::Error(e) => {
                log::error!("Error: {}", e);
            }
            StreamResult::ProcessFail(ProcessFailKind::Demod(_)) => {
                demod_counter += 1;
            }
            StreamResult::ProcessFail(_kind) => {}
        }
    }

    println!("done, demod_counter = {}", demod_counter);

    *rx[0].running.lock().unwrap() = false;

    Ok(())
}

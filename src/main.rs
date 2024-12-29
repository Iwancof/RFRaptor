use hydro_strike::*;

use clap::Parser;

use anyhow::Context;

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

    let sb = signalbool::SignalBool::new(&[signalbool::Signal::SIGINT], signalbool::Flag::Restart)?;
    for packet in rx[0].start_rx()? {
        if let crate::bluetooth::PacketInner::Advertisement(ref adv) = packet.packet.inner {
            log::info!(
                "{}",
                packet.bytes_packet.raw.unwrap().raw.unwrap().rssi_average
            );
            log::info!("{}", adv);
        }

        if sb.caught() {
            break;
        }
    }

    *rx[0].running.lock().unwrap() = false;

    Ok(())
}

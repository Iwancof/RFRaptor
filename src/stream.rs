#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
struct SdrIdx(usize);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct BluetoothChannel {
    blch: u32, // private
}

impl BluetoothChannel {
    fn from_freq(freq: u32) -> Self {
        BluetoothChannel {
            blch: (freq - 2402) / 2,
        }
    }
    fn to_freq(self) -> u32 {
        2402 + 2 * self.blch
    }
}

type RxChannelSender = (
    BluetoothChannel,
    std::sync::mpsc::Sender<Vec<num_complex::Complex<f32>>>,
);
type RxChannelReceiver = (
    SdrIdx,
    std::sync::mpsc::Receiver<Vec<num_complex::Complex<f32>>>,
);

use std::collections::HashMap;

use anyhow::Context;

#[derive(Debug)]
pub enum ProcessFailKind {
    Catcher,
    TooShort,
    #[allow(dead_code)]
    Demod(anyhow::Error),
    Bitops,
    Bluetooth,
}

pub trait Stream {
    fn start_rx(&mut self) -> anyhow::Result<RxStream<crate::bluetooth::Bluetooth>>;
}

impl crate::device::Device {
    fn prepare_pfbch2_fsk_mpsc(
        &self,
    ) -> (
        HashMap<SdrIdx, RxChannelSender>,
        HashMap<BluetoothChannel, RxChannelReceiver>,
    ) {
        let mut sdridx_to_sender: HashMap<SdrIdx, RxChannelSender> = HashMap::new();
        let mut blch_to_receiver: HashMap<BluetoothChannel, RxChannelReceiver> = HashMap::new();

        let channel_half = self.config.num_channels as isize / 2;

        for (sdr_idx, (tx, rx)) in (0..self.config.num_channels)
            .map(|_| std::sync::mpsc::channel::<Vec<num_complex::Complex<f32>>>())
            .enumerate()
        {
            let sdr_idx_isize = sdr_idx as isize;
            let freq_offset = if sdr_idx_isize < channel_half {
                sdr_idx_isize
            } else {
                sdr_idx_isize - self.config.num_channels as isize
            };

            let freq = self.config.freq_mhz as isize + freq_offset;

            if freq & 1 == 0 && (2402..=2480).contains(&freq) {
                let blch = BluetoothChannel::from_freq(freq as u32);

                sdridx_to_sender.insert(SdrIdx(sdr_idx), (blch, tx));
                blch_to_receiver.insert(blch, (SdrIdx(sdr_idx), rx));
            }
        }

        (sdridx_to_sender, blch_to_receiver)
    }

    // for SoapyHackRF
    fn check_remain_count(raw: &soapysdr::Device) -> anyhow::Result<()> {
        if let Some(remain_count) = raw
            .channel_info(soapysdr::Direction::Rx, 0)
            .context("channel_info")?
            .get("buffer_count")
        {
            let remain_count = remain_count.parse::<usize>()?;
            log::trace!("remain_count: {}", remain_count);
        }

        Ok(())
    }

    fn wake_channelizer(
        &mut self,
        sdridx_to_sender: HashMap<SdrIdx, RxChannelSender>,
        on_error: impl Fn(anyhow::Error) + 'static + Send + Clone,
    ) -> anyhow::Result<()> {
        let config = self.config.clone();
        let raw = self.raw.clone();
        let running = self.running.clone();

        let mut read_stream = self.raw.rx_stream_args::<num_complex::Complex<f32>, _>(
            &[self.config.channels],
            "buffers=65535",
        )?;

        // let mut channelizer = crate::channelizer::Channelizer::new(config.num_channels, 4, 0.75);
        let mut channelizer = crate::channelizer::Channelizer::new(config.num_channels);
        // log::trace!("wake_channelizer\n{}", channelizer);

        let mut fft_result: Vec<Vec<num_complex::Complex<f32>>> = (0..config.num_channels)
            .map(|_| Vec::with_capacity(131072 / (config.num_channels / 2)))
            .collect::<Vec<_>>();

        let mut buffer =
            vec![num_complex::Complex::default(); read_stream.mtu()?].into_boxed_slice();

        // std::thread::spawn(move || {
        let _ = std::thread::Builder::new()
            .name("wake_channelizer".to_string())
            .spawn(move || {
                if let Err(e) = read_stream.activate(None) {
                    on_error(e.into());
                    return;
                }

                let ret: anyhow::Result<()> = (|| loop {
                    let _read = read_stream
                        .read(&mut [&mut buffer[..]], 1_000_000)
                        .context("wake_channelizer(read)")?;

                    Self::check_remain_count(&raw)?;

                    for fft in fft_result.iter_mut() {
                        fft.clear();
                    }

                    for chunk in buffer.chunks_exact_mut(config.num_channels / 2) {
                        for (sdridx, fft) in channelizer.channelize(chunk).iter().enumerate() {
                            if sdridx_to_sender.contains_key(&SdrIdx(sdridx)) {
                                fft_result[sdridx].push(*fft);
                            }
                        }
                    }

                    for (sdridx, fft) in fft_result.iter().enumerate() {
                        if let Some((_blch, tx)) = sdridx_to_sender.get(&SdrIdx(sdridx)) {
                            tx.send(fft.clone()).context("wake_channelizer(send)")?;
                        }
                    }

                    if !*running.lock().expect("failed to lock") {
                        anyhow::bail!("Interrupted");
                    }
                })();

                *running.lock().expect("failed to lock") = false;

                if let Err(e) = read_stream.deactivate(None) {
                    on_error(e.into());
                }

                if let Err(e) = ret {
                    on_error(e);
                }
            });

        Ok(())
    }

    fn catch_and_process(
        &mut self,
        rxs: HashMap<BluetoothChannel, RxChannelReceiver>,

        sender: impl Fn(crate::bluetooth::Bluetooth) + 'static + Send + Clone,
        process_fail: impl Fn(ProcessFailKind) + 'static + Send + Clone,
        on_error: impl Fn(anyhow::Error) + 'static + Send + Clone,
    ) -> anyhow::Result<()> {
        let sample_rate = self.config.sample_rate;
        let num_channels = self.config.num_channels;

        for (ble_ch_idx, sdr_idx_rx) in rxs.into_iter() {
            let freq = ble_ch_idx.to_freq();

            let (_sdr_idx, rx) = sdr_idx_rx;

            let sender = sender.clone();
            let process_fail = process_fail.clone();
            let on_error = on_error.clone();

            std::thread::spawn(move || {
                let mut burst = crate::burst::Burst::new();
                let mut fsk = crate::fsk::FskDemod::new(sample_rate as _, num_channels);

                loop {
                    let channelized_values = match rx.recv().context("catch_and_process(recv)") {
                        Ok(v) => v,
                        Err(e) => {
                            on_error(e);
                            break;
                        }
                    };

                    for s in channelized_values {
                        let ret: Result<(), ProcessFailKind> = (|| {
                            let packet = burst
                                // .catcher(s / num_channels as f32)
                                .catcher(s)
                                .ok_or(ProcessFailKind::Catcher)?;

                            if packet.data.len() < 132 {
                                return Err(ProcessFailKind::TooShort);
                            }

                            let demodulated =
                                fsk.demodulate(packet).map_err(ProcessFailKind::Demod)?;

                            let byte_packet =
                                crate::bitops::fsk_to_packet(demodulated, freq as usize)
                                    .map_err(|_| ProcessFailKind::Bitops)?;

                            if !byte_packet.remain_bits.is_empty() {
                                log::trace!("remain bits: {:?}", byte_packet.remain_bits);
                            }

                            let bt =
                                crate::bluetooth::Bluetooth::from_bytes(byte_packet, freq as usize)
                                    .map_err(|_| ProcessFailKind::Bluetooth)?;

                            sender(bt);

                            Ok(())
                        })();

                        if let Err(e) = ret {
                            process_fail(e);
                        }
                    }
                }
            });
        }

        Ok(())
    }

    pub fn start_rx_with_error(&mut self) -> anyhow::Result<RxStream<StreamResult>> {
        // sink/source Bluetooth Packet

        let (packet_sink, packet_source) = std::sync::mpsc::channel();
        *self.running.lock().expect("failed to lock") = true;

        let (sdridx_to_sender, blch_to_receiver) = self.prepare_pfbch2_fsk_mpsc();

        let ps1 = packet_sink.clone();

        self.wake_channelizer(sdridx_to_sender, move |e| {
            let _ = ps1.send(StreamResult::Error(e));
        })?;

        let ps2 = packet_sink.clone();
        let ps3 = packet_sink.clone();
        let ps4 = packet_sink.clone();

        self.catch_and_process(
            blch_to_receiver,
            move |packet| {
                let _ = ps2.send(StreamResult::Packet(Box::new(packet)));
            },
            move |fail| {
                let _ = ps3.send(StreamResult::ProcessFail(fail));
            },
            move |e| {
                let _ = ps4.send(StreamResult::Error(e));
            },
        )?;

        Ok(RxStream {
            source: packet_source,
        })
    }
}

impl Stream for crate::device::Device {
    fn start_rx(&mut self) -> anyhow::Result<RxStream<crate::bluetooth::Bluetooth>> {
        // sink/source Bluetooth Packet

        let (packet_sink, packet_source) = std::sync::mpsc::channel();
        *self.running.lock().expect("failed to lock") = true;

        let (sdridx_to_sender, blch_to_receiver) = self.prepare_pfbch2_fsk_mpsc();

        self.wake_channelizer(sdridx_to_sender, |_e| {})?;
        self.catch_and_process(
            blch_to_receiver,
            move |packet| {
                let _ = packet_sink.send(packet);
            },
            |_fail| {},
            |_e| {},
        )?;

        Ok(RxStream {
            source: packet_source,
        })
    }
}

pub enum StreamResult {
    Packet(Box<crate::bluetooth::Bluetooth>),
    Error(anyhow::Error),
    ProcessFail(ProcessFailKind),
}

pub struct RxStream<ReceiveItem> {
    source: std::sync::mpsc::Receiver<ReceiveItem>,
}

impl<T> std::iter::Iterator for RxStream<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.source.recv().ok()
    }
}

type RxChannelSender = (
    usize,
    std::sync::mpsc::Sender<Vec<num_complex::Complex<f32>>>,
);
type RxChannelReceiver = (
    usize,
    std::sync::mpsc::Receiver<Vec<num_complex::Complex<f32>>>,
);

impl crate::device::Device {
    fn prepare_pfbch2_fsk_mpsc(
        &self,
    ) -> (Vec<Option<RxChannelSender>>, Vec<Option<RxChannelReceiver>>) {
        let mut sdridx_to_sender: Vec<Option<RxChannelSender>> = vec![];
        let mut blch_to_receiver: Vec<Option<RxChannelReceiver>> = vec![];

        for _ in 0..self.config.num_channels {
            sdridx_to_sender.push(None);
        }
        for _ in 0..96 {
            blch_to_receiver.push(None);
        }

        for (sdr_idx, (tx, rx)) in (0..self.config.num_channels)
            .map(|_| std::sync::mpsc::channel::<Vec<num_complex::Complex<f32>>>())
            .enumerate()
        {
            let sdr_idx_isize = sdr_idx as isize;
            let freq = self.config.freq_mhz as isize
                + if sdr_idx_isize < (self.config.num_channels as isize / 2) {
                    sdr_idx_isize
                } else {
                    sdr_idx_isize - self.config.num_channels as isize
                };

            if freq & 1 == 0 && (2402..=2480).contains(&freq) {
                let blch = ((freq - 2402) / 2) as usize;

                sdridx_to_sender[sdr_idx] = Some((blch, tx));
                blch_to_receiver[blch] = Some((sdr_idx, rx));
            }
        }

        (sdridx_to_sender, blch_to_receiver)
    }

    fn wake_channelizer(
        &mut self,
        sdridx_to_sender: Vec<Option<RxChannelSender>>,
    ) -> anyhow::Result<()> {
        let config = self.config.clone();
        let raw = self.raw.clone();
        let running = self.running.clone();

        let mut read_stream = self.raw.rx_stream_args::<num_complex::Complex<f32>, _>(
            &[self.config.channels],
            "buffers=65535",
        )?;

        std::thread::spawn(move || {
            let mut channelizer = crate::channelizer::Channelizer::new(config.num_channels);
            log::info!("wake_channelizer\n{}", channelizer);

            let mut fft_result: Vec<Vec<num_complex::Complex<f32>>> = (0..config.num_channels)
                .map(|_| Vec::with_capacity(131072 / (config.num_channels / 2)))
                .collect::<Vec<_>>();

            // fixed size buffer
            let mut buffer =
                vec![num_complex::Complex::default(); read_stream.mtu()?].into_boxed_slice();
            read_stream.activate(None)?;
            '_outer: for _ in 0.. {
                let _read = read_stream.read(&mut [&mut buffer[..]], 1_000_000)?;
                // println!("read: {}", _read);
                // println!("{:?}", &buffer[_read-3..]);
                // assert_eq!(read, buffer.len());

                if let Some(remain_count) = raw
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
                    for (sdridx, fft) in channelizer.channelize(chunk).iter().enumerate() {
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

                if !*running.lock().expect("failed to lock") {
                    break '_outer;
                }
            }

            read_stream.deactivate(None)?;

            anyhow::Result::<()>::Ok(())
        });

        Ok(())
    }

    fn create_catcher_threads(
        &mut self,
        rxs: Vec<Option<RxChannelReceiver>>,
        sender: std::sync::mpsc::Sender<crate::bluetooth::Bluetooth>,
    ) -> anyhow::Result<()> {
        let sample_rate = self.config.sample_rate;
        let num_channels = self.config.num_channels;

        for (ble_ch_idx, sdr_idx_rx) in rxs
            .into_iter()
            .enumerate()
            .filter(|(_, sdr_idx_rx)| sdr_idx_rx.is_some())
        {
            let freq = 2402 + 2 * ble_ch_idx as u32;

            let (_sdr_idx, rx) = sdr_idx_rx.unwrap();
            let sender = sender.clone();
            std::thread::spawn(move || {
                let mut burst = crate::burst::Burst::new();
                let mut fsk = crate::fsk::FskDemod::new(sample_rate as _, num_channels);

                #[derive(Debug)]
                enum ErrorKind {
                    Catcher,
                    TooShort,
                    Demod(anyhow::Error),
                    Bitops,
                    Bluetooth,
                }

                loop {
                    let Ok(received) = rx.recv() else {
                        break;
                    };

                    for s in received {
                        let ret: Result<(), ErrorKind> = (|| {
                            let packet = burst
                                // .catcher(s / num_channels as f32)
                                .catcher(s)
                                .ok_or(ErrorKind::Catcher)?;

                            if packet.data.len() < 132 {
                                return Err(ErrorKind::TooShort);
                            }

                            let demodulated = fsk.demodulate(packet).map_err(ErrorKind::Demod)?;

                            let byte_packet =
                                crate::bitops::bits_to_packet(&demodulated.bits, freq as usize)
                                    .map_err(|_| ErrorKind::Bitops)?;

                            if !byte_packet.remain_bits.is_empty() {
                                log::trace!("remain bits: {:?}", byte_packet.remain_bits);
                            }

                            let bt =
                                crate::bluetooth::Bluetooth::from_bytes(byte_packet, freq as usize)
                                    .map_err(|_| ErrorKind::Bluetooth)?;

                            sender.send(bt).unwrap();

                            Ok(())
                        })();

                        let Err(kind) = ret else {
                            continue;
                        };

                        match kind {
                            ErrorKind::Catcher => {
                                //
                            }
                            ErrorKind::TooShort => {
                                //
                            }
                            ErrorKind::Demod(_d) => {
                                // static DEMOD_FAIL_COUNTER: std::sync::LazyLock<
                                //     std::sync::atomic::AtomicUsize,
                                // > = const {
                                //     std::sync::LazyLock::new(|| std::sync::atomic::AtomicUsize::new(0))
                                // };
                                // // log::error!("failed to demodulate: {}", d);
                                // let count = DEMOD_FAIL_COUNTER
                                //     .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                // println!("failed to demodulate: {} (count: {})", d, count);
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

        Ok(())
    }

    pub fn start_rx(&mut self) -> anyhow::Result<RxStream<crate::bluetooth::Bluetooth>> {
        // sink/source Bluetooth Packet
        let (sender, receiver) = std::sync::mpsc::channel();
        *self.running.lock().expect("failed to lock") = true;

        let (sdridx_to_sender, blch_to_receiver) = self.prepare_pfbch2_fsk_mpsc();

        self.wake_channelizer(sdridx_to_sender)?;
        self.create_catcher_threads(blch_to_receiver, sender)?;

        Ok(RxStream { source: receiver })
    }

    /*
    pub fn start_rx_with_error(
        &mut self,
    ) -> RxStream<'_, anyhow::Result<crate::bluetooth::Bluetooth>> {
        let (sender, receiver) = std::sync::mpsc::channel();

        // TODO

        RxStream {
            source: receiver,
        }
    }
    */
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

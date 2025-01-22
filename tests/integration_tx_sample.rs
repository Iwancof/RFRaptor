use rfraptor::*;

#[test]
fn fsk_bits() {
    let num_channels = 16;

    let mut demodulater = fsk::FskDemod::new(20e6, num_channels as _);
    let mut modulater = fsk::FskMod::new(20e6, num_channels as _);

    let original_bytes = (0..0x10).map(|i| i as u8).collect::<Vec<_>>();

    let bits = bitops::packet_to_bits(&original_bytes, 2427, 0xdeadbeef);
    let modulated = modulater.modulate(&bits).unwrap();

    let demodulated = demodulater.demodulate_signal(&modulated).unwrap();
    let bytes = bitops::bits_to_packet(&demodulated.bits, 2427).unwrap();

    assert_eq!(bytes.bytes[5], 0x10);
    assert_eq!(bytes.bytes[6..][..0x10], original_bytes);
}

struct Wave {
    idx: usize,
    gamma: f32,
}

impl Iterator for Wave {
    type Item = num_complex::Complex32;

    fn next(&mut self) -> Option<Self::Item> {
        let ret = Some(
            self.gamma
                * num_complex::Complex32::new(
                    0.,
                    2. * std::f32::consts::PI * 0.0193 * self.idx as f32,
                )
                .exp(),
        );

        self.idx += 1;

        ret
    }
}

#[test]
fn pfbch_fsk_bits() {
    let num_channels = 16;

    let mut channelizer = channelizer::Channelizer::new(num_channels);
    let mut synthesizer = channelizer::Synthesizer::new(num_channels);

    let mut burst = burst::Burst::default();

    let mut demodulater = fsk::FskDemod::new(20e6, num_channels as _);
    let mut modulater = fsk::FskMod::new(20e6, num_channels as _);

    let original_bytes = (0..0x10).map(|i| i as u8).collect::<Vec<_>>();

    let bits = bitops::packet_to_bits(&original_bytes, 2427, 0xdeadbeef);
    let modulated = modulater.modulate(&bits).unwrap();

    let mut rf = vec![];

    for m in []
        .into_iter()
        .chain(
            Wave {
                idx: 0,
                gamma: 1e-4,
            }
            .take(100),
        )
        .chain(
            Wave {
                idx: 100,
                gamma: 0.0035,
            }
            .take(16),
        )
        .chain(modulated.into_iter())
        .chain(
            Wave {
                idx: 0,
                gamma: 1e-3,
            }
            .take(200),
        )
    {
        let mut signals = vec![num_complex::Complex32::new(0., 0.); num_channels];
        signals[num_channels / 2] = m;

        let synthesized = synthesizer.synthesize(&signals);
        rf.extend_from_slice(synthesized);
    }

    let mut demodulated = vec![];
    for chunk in rf.chunks(num_channels / 2) {
        let channelized = channelizer.channelize(chunk);

        // demodulated.extend_from_slice(&syn);
        demodulated.push(channelized[num_channels / 2]);
    }

    for d in demodulated.iter() {
        let catch = burst.catcher(*d);

        if let Some(packet) = catch {
            let demodulated = demodulater.demodulate(packet).unwrap();
            println!("{:?}", demodulated.bits);

            let mut tmp = vec![];
            tmp.extend_from_slice(&demodulated.bits);

            // let bytes = bitops::bits_to_packet(&demodulated.bits, 2427).unwrap();
            let bytes = bitops::bits_to_packet(&tmp, 2427).unwrap();
            assert_eq!(bytes.aa, 0xdeadbeef);
            assert_eq!(bytes.bytes[5], 0x10);
            assert_eq!(bytes.bytes[6..][..0x10], original_bytes);

            return;
        }
    }

    panic!("no packet found");
}

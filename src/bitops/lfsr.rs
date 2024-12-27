#[derive(Debug)]
pub struct LFSR0221 {
    state: u8,
}

impl LFSR0221 {
    pub fn from_freq(freq: usize) -> Self {
        fn freq_to_channel(freq: usize) -> u8 {
            let phys_channel = (freq - 2402) / 2;
            if phys_channel == 0 {
                return 37;
            }
            if phys_channel == 12 {
                return 38;
            }
            if phys_channel == 39 {
                return 39;
            }
            if phys_channel < 12 {
                return (phys_channel - 1) as _;
            }
            (phys_channel - 2) as _
        }

        let channel = freq_to_channel(freq);

        Self::from_ch(channel)
    }

    pub fn from_ch(channel: u8) -> Self {
        assert!(channel <= 0b111111);

        Self {
            state: channel | 0b1000000,
        }
    }

    pub fn next_white(&mut self) -> u8 {
        // LFSR: g(D) = D^7 + D^4 + 1 ( 221 in octal )
        let bit = self.state & 1;

        self.state >>= 1;
        self.state ^= if bit == 1 { 0b1000100 } else { 0 };

        bit
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn ch_zero() {
        let mut lfsr = super::LFSR0221::from_ch(0);

        let take = 20;

        let mut white = vec![];

        for _ in 0..take {
            white.push(lfsr.next_white());
        }

        let expect: Vec<u8> = vec![
            0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0,
            0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 1,
        ]
        .into_iter()
        .take(take)
        .collect();

        assert_eq!(white, expect);
    }

    #[test]
    fn uptest_lsfr() {
        let raw_bits = vec![0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1]; // random bits

        let mut lfsr = super::LFSR0221::from_ch(0);
        let mut whited_bits = vec![];

        for b in raw_bits.iter() {
            whited_bits.push(b ^ lfsr.next_white());
        }

        let mut lfsr = super::LFSR0221::from_ch(0);
        let mut dewhited_bits = vec![];

        for b in whited_bits.iter() {
            dewhited_bits.push(b ^ lfsr.next_white());
        }

        assert_eq!(raw_bits, dewhited_bits);
    }
}

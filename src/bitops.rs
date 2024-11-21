mod bitparser;
mod lfsr;

use anyhow::{anyhow, bail, Result};
use bitparser::*;

#[derive(Debug, Clone)]
pub struct BytePacket {
    #[allow(unused)]
    pub bytes: Vec<u8>,
    #[allow(unused)]
    pub aa: u32,
    #[allow(unused)]
    pub freq: usize,
    #[allow(unused)]
    pub delta: i64,
    #[allow(unused)]
    pub offset: usize,
}

pub fn bits_to_packet<'a>(bits: &'a [u8], freq: usize) -> Result<(&'a [u8], BytePacket)> {
    use zerocopy::FromBytes;

    let bits_len = bits.len() as i64;

    let Ok((bits, lap)) = Lap::parse(bits) else {
        bail!("failed to parse lap")
    };

    if !lap.is_valid_as_ble() {
        bail!("lap is not valid");
    }

    let Ok((bits, _)) = Preamble::parse(bits) else {
        bail!("failed to parse preamble");
    };

    let mut found_data = useful_number::updatable_num::UpdateToMinI64WithData::new();
    for offset in 0..3 {
        let mut bits = &bits[offset..];

        let mut whitening = lfsr::LFSR0221::from_freq(freq);
        let mut bytes = Vec::new();

        for _ in 0..4 {
            let Ok((remain, byte)) = RawByte::parse(bits) else {
                bail!("bit starvation");
            };

            bits = remain;
            bytes.push(byte.byte);
        }

        while let Ok((remain, WhitedByte { byte })) = WhitedByte::parse(bits, &mut whitening) {
            bits = remain;
            bytes.push(byte);
        }

        let packet_length = 8 + 32 + 16 + bytes[5] as i64 * 8 + 24;

        let delta = bits_len - packet_length;
        if delta <= 0 {
            continue;
        }

        found_data.update(delta, (bytes, bits, offset));
    }

    let Some((delta, (bytes, remain_bits, offset))) = found_data.take() else {
        bail!("valid length data not found");
    };

    if 20 <= delta {
        bail!("delta is too bit {}", delta);
    }

    let Ok(aa) = u32::ref_from_bytes(&bytes[0..4]) else {
        bail!("bytes is too small to get AA");
    };

    let aa = *aa;

    Ok((
        remain_bits,
        BytePacket {
            bytes,
            aa,

            offset,
            delta,
            freq,
        },
    ))
}

#[cfg(test)]
mod test {
    #[test]
    fn bits_to_packet() {
        let bits = vec![
            0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0,
            0, 0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1,
            1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0,
            1, 1, 0, 1, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0,
            0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 1,
            1, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0,
            0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0,
            1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1,
            1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0,
            1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0,
            0, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0,
            0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0,
            0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1,
        ];

        let (remain, byte_packet) = super::bits_to_packet(&bits, 2426).unwrap();

        assert_eq!(byte_packet.aa, 0x8e89bed6);
        assert_eq!(byte_packet.offset, 2);
        assert_eq!(byte_packet.delta, 6);

        assert_eq!(remain.len(), byte_packet.delta as usize);
    }
}

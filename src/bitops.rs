mod lfsr;
mod bitparser;

use bitparser::*;
use anyhow::{Result, anyhow};

#[derive(Debug, Clone)]
pub struct BytePacket {
    pub bytes: Vec<u8>,
    pub aa: u32,
    pub freq: usize,
    pub delta: i64,
    pub offset: usize,
}

pub fn bits_to_packet<'a>(bits: &'a [u8], freq: usize) -> Result<(&'a [u8], BytePacket)> {
    use zerocopy::FromBytes;

    let bits_len = bits.len() as i64;

    let (bits, lap) = Lap::parse(bits).map_err(|_| anyhow!("failed to parse lap"))?;
    if !lap.is_valid_as_ble() {
        return Err(anyhow!("lap is not valid"));
    }

    let (bits, _) = Preamble::parse(bits).map_err(|_| anyhow!("failed to parse preamble"))?;

    let mut found_data = useful_number::updatable_num::UpdateToMinI64WithData::new();
    for offset in 0..3 {
        let mut bits = &bits[offset..];

        let mut whitening = lfsr::LFSR0221::from_freq(freq);
        let mut bytes = Vec::new();

        for _ in 0..4 {
            let (remain, byte) = RawByte::parse(bits).map_err(|_| anyhow!("bit starvation"))?;

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

    let (delta, (bytes, remain_bits, offset)) = found_data
        .take()
        .ok_or(anyhow!("valid length data not found"))?;

    if 20 <= delta {
        anyhow::bail!("delta is too bit {}", delta);
    }

    let aa =
        *u32::ref_from_bytes(&bytes[0..4]).map_err(|_| anyhow!("bytes is too small to get AA"))?;

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

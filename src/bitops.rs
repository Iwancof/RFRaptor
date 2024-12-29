mod bitparser;
mod lfsr;

use anyhow::{bail, Result};
use bitparser::*;

#[derive(Debug, Clone)]
pub struct BytePacket {
    #[allow(unused)]
    pub raw: Option<crate::fsk::Packet>,

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

    #[allow(unused)]
    pub remain_bits: Vec<u8>,
}

pub fn fsk_to_packet(packet: crate::fsk::Packet, freq: usize) -> Result<BytePacket> {
    let bits = bits_to_packet(&packet.bits, freq)?;

    Ok(BytePacket {
        raw: Some(packet),
        ..bits
    })
}

pub fn bits_to_packet(bits: &[u8], freq: usize) -> Result<BytePacket> {
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

    Ok(BytePacket {
        raw: None,

        bytes,
        aa,

        offset,
        delta,
        freq,
        remain_bits: remain_bits.to_vec(),
    })
}

pub fn packet_to_bits(bytes: &[u8], freq: usize, aa: u32) -> Vec<u8> {
    let mut bits = Vec::new();

    Preamble::encode(&mut bits);

    // offset = 2
    bits.push(0);
    bits.push(0);

    for b in aa.to_le_bytes() {
        RawByte { byte: b }.encode(&mut bits);
    }

    let mut whitening = lfsr::LFSR0221::from_freq(freq);

    let header_padding = 0;
    let length = bytes.len() as u8;

    WhitedByte {
        byte: header_padding,
    }
    .encode(&mut bits, &mut whitening);
    WhitedByte { byte: length }.encode(&mut bits, &mut whitening);

    for b in bytes {
        WhitedByte { byte: *b }.encode(&mut bits, &mut whitening);
    }

    // add CRC
    for _i in 0..3 {
        WhitedByte { byte: 0 }.encode(&mut bits, &mut whitening); // FIXME
    }

    // add some garbages
    bits.push(0);
    bits.push(0);
    bits.push(0);
    bits.push(0);

    bits
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

        let byte_packet = super::bits_to_packet(&bits, 2426).unwrap();

        assert_eq!(byte_packet.aa, 0x8e89bed6);
        assert_eq!(byte_packet.offset, 2);
        assert_eq!(byte_packet.delta, 6);

        assert_eq!(byte_packet.remain_bits.len(), byte_packet.delta as usize);
    }

    #[test]
    fn uptest_bytes() {
        let bytes = b"hello world!";

        let bits = super::packet_to_bits(bytes, 2426, 0x8e89bed6);

        let byte_packet = super::bits_to_packet(&bits, 2426).unwrap();

        assert_eq!(byte_packet.aa, 0x8e89bed6);
        assert_eq!(byte_packet.offset, 2);

        assert_eq!(byte_packet.delta, 4);
        assert_eq!(byte_packet.remain_bits.len(), 4);
    }
}

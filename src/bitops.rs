// mod lap;
// mod preamble; mod aa;
// mod whitening;
mod lfsr;

use anyhow::Result;

use nom::{bytes::complete::take, error::ErrorKind};

#[derive(Debug, Clone)]
pub struct BytePacket {
    pub bytes: Vec<u8>,
    pub aa: u32,
    pub freq: usize,
    pub delta: i64,
    pub offset: usize,
}

pub fn bits_to_packet<'a>(bits: &'a [u8], freq: usize) -> Result<(&'a [u8], BytePacket)> {
    use anyhow::anyhow;
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

pub struct Lap {
    lap: Option<u32>,
}

impl Lap {
    fn parse<'a>(input: &[u8]) -> nom::IResult<&[u8], Self> {
        use core::mem::MaybeUninit;
        use libbtbb_sys::btbb_packet;

        let mut btbb_packet = MaybeUninit::<btbb_packet>::zeroed();
        let ret = unsafe {
            libbtbb_sys::btbb_find_ac(
                input.as_ptr() as _,
                input.len() as _,
                libbtbb_sys::LAP_ANY,
                1,
                (&mut btbb_packet.as_mut_ptr()) as _,
            )
        };

        if ret < 0 {
            return Ok((input, Self { lap: None }));
        }

        // btbb_packet is valid
        let btbb_packet = unsafe { btbb_packet.assume_init() };

        return Ok((
            input,
            Self {
                lap: Some(btbb_packet.LAP),
            },
        ));
    }

    fn is_valid_as_ble(&self) -> bool {
        if let Some(lap) = self.lap {
            return lap == 0xffffffff;
        }

        return true;
    }
}

pub struct Preamble {}

impl Preamble {
    fn parse(input: &[u8]) -> nom::IResult<&[u8], Self> {
        let (remain, took) = take(6u8)(input)?;

        let mut fail = false;

        fail |= took[0] != took[2]; // fail
        fail |= took[1] != took[3]; // fail
        fail |= took[2] != took[4]; // fail
        fail |= took[3] != took[5]; // fail

        if fail {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                ErrorKind::Fail,
            )));
        }

        Ok((remain, Self {}))
    }
}

pub struct RawByte {
    byte: u8,
}

impl RawByte {
    fn parse<'a>(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (remain, raw_bits) = take(8u8)(input)?;

        let mut byte = 0;
        for (i, b) in raw_bits.iter().enumerate() {
            byte |= b << i;
        }

        Ok((remain, Self { byte }))
    }
}

pub struct WhitedByte {
    byte: u8,
}

impl WhitedByte {
    fn parse<'a>(input: &'a [u8], lsfr: &mut lfsr::LFSR0221) -> nom::IResult<&'a [u8], Self> {
        let (remain, raw_bits) = take(8u8)(input)?;

        let mut byte = 0;
        for (i, b) in raw_bits.iter().enumerate() {
            byte |= (b ^ lsfr.next_white()) << i;
        }

        Ok((remain, Self { byte }))
    }
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

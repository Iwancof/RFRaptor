use nom::{bytes::complete::take, error::ErrorKind};
use super::lfsr;

#[derive(Debug)]
pub struct Lap {
    pub lap: Option<u32>,
}

impl Lap {
    pub fn parse(input: &[u8]) -> nom::IResult<&[u8], Self> {
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

        Ok((
            input,
            Self {
                /*
                lap: Some(btbb_packet.LAP),
                */
                lap: Some(
                    unsafe {
                        libbtbb_sys::btbb_packet_get_lap(&btbb_packet)
                    }
                )
            },
        ))
    }

    pub fn is_valid_as_ble(&self) -> bool {
        if let Some(lap) = self.lap {
            return lap == 0xffffffff;
        }

        true
    }
}

#[derive(Debug)]
pub struct Preamble {}

impl Preamble {
    pub fn parse(input: &[u8]) -> nom::IResult<&[u8], Self> {
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

#[derive(Debug)]
pub struct RawByte {
    pub byte: u8,
}

impl RawByte {
    pub fn parse(input: &[u8]) -> nom::IResult<&[u8], Self> {
        let (remain, raw_bits) = take(8u8)(input)?;

        let mut byte = 0;
        for (i, b) in raw_bits.iter().enumerate() {
            byte |= b << i;
        }

        Ok((remain, Self { byte }))
    }
}

#[derive(Debug)]
pub struct WhitedByte {
    pub byte: u8,
}

impl WhitedByte {
    pub fn parse<'a>(input: &'a [u8], lsfr: &mut lfsr::LFSR0221) -> nom::IResult<&'a [u8], Self> {
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
    use super::*;

    #[test]
    fn preamble_ok() {
        let input = [0, 1, 0, 1, 0, 1];
        let (remain, _) = Preamble::parse(&input).expect("parse failed");

        assert_eq!(remain.len(), 0);
    }

    #[test]
    fn preamble_fail() {
        let input = [0, 1, 0, 1, 0, 0];
        Preamble::parse(&input).expect_err("parse ok");
    }

    #[test]
    fn raw_byte() {
        let input = [0, 1, 0, 1, 0, 1, 0, 1];
        let (remain, raw_byte) = RawByte::parse(&input).expect("parse failed");

        assert_eq!(remain.len(), 0);
        assert_eq!(raw_byte.byte, 0b10101010);
    }
}

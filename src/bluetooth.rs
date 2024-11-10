// use ice9_bindings::*;

use nom::{bytes::complete::take, number::complete::le_u32, IResult};

use crate::bitops::BytePacket;

// TODO: いい感じに実装する
pub struct Bluetooth {
    pub bytes_packet: BytePacket,

    #[allow(unused)]
    pub packet: BluetoothPacket,

    #[allow(unused)]
    pub remain: Vec<u8>,

    #[allow(unused)]
    pub freq: usize,
}

pub enum DecodeError {
    #[allow(unused)]
    FoundClassic(u32),

    #[allow(unused)]
    PacketNotFound,
}

pub enum BluetoothPacket {
    Advertisement(Advertisement),
    Unimplemented(u32),
}

pub struct Advertisement {
    pub pdu_type: PDUType,
    pub length: u8,
    pub address: MacAddress,
}

pub struct MacAddress {
    pub address: [u8; 6],
}

pub enum PDUType {
    AdvInd,
    AdvDirectInd,
    AdvNonconnInd,
    ScanReq,
    ScanRsp,
    ConnectReq,
    AdvScanInd,
    Unknown(u8),
}

impl Bluetooth {
    pub fn from_bytes(byte_packet: BytePacket , freq: usize) -> Result<Self, DecodeError> {
        let (remain, packet) = BluetoothPacket::from_bytes(byte_packet.bytes.as_ref()).unwrap();
        // FIXME: unwrap will panic if slice is too short

        Ok(Self {
            bytes_packet: byte_packet.clone(),
            packet,
            remain: remain.to_vec(),
            freq,
        })
    }

    /*
    pub fn from_packet(packet: &_packet_t, freq: u32) -> Result<Self, DecodeError> {
        let slice = unsafe { std::slice::from_raw_parts(packet.bits, packet.bits_len as usize) };

        Self::from_bits(&slice.to_vec(), freq as usize)
    }
    */
}

impl PDUType {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte & 0b1111 {
            0b0000 => Some(PDUType::AdvInd),
            0b0001 => Some(PDUType::AdvDirectInd),
            0b0010 => Some(PDUType::AdvNonconnInd),
            0b0011 => Some(PDUType::ScanReq),
            0b0100 => Some(PDUType::ScanRsp),
            0b0101 => Some(PDUType::ConnectReq),
            0b0110 => Some(PDUType::AdvScanInd),
            x => Some(PDUType::Unknown(x)),
        }
    }
}

impl BluetoothPacket {
    fn from_bytes(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, access_address) = le_u32(input)?;

        match access_address {
            0x8E89BED6 => {
                let (input, adv) = Advertisement::from_bytes(input)?;
                Ok((input, BluetoothPacket::Advertisement(adv)))
            }
            other => Ok((input, BluetoothPacket::Unimplemented(other))),
        }
    }
}

impl Advertisement {
    fn from_bytes(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, pdu_type) = take(1u8)(input)?;
        let pdu_type = PDUType::from_byte(pdu_type[0]).unwrap();

        let (input, length) = take(1u8)(input)?;
        let length = length[0];

        let (input, address) = MacAddress::from_bytes(input)?;

        Ok((
            input,
            Advertisement {
                pdu_type,
                length,
                address,
            },
        ))
    }
}

impl MacAddress {
    fn from_bytes(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, address) = take(6u8)(input)?;

        Ok((
            input,
            MacAddress {
                address: [
                    address[0], address[1], address[2], address[3], address[4], address[5],
                ],
            },
        ))
    }
}

impl core::fmt::Display for MacAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.address[5],
            self.address[4],
            self.address[3],
            self.address[2],
            self.address[1],
            self.address[0]
        )
    }
}

impl core::fmt::Display for PDUType {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            PDUType::AdvInd => write!(f, "ADV_IND"),
            PDUType::AdvDirectInd => write!(f, "ADV_DIRECT_IND"),
            PDUType::AdvNonconnInd => write!(f, "ADV_NONCONN_IND"),
            PDUType::ScanReq => write!(f, "SCAN_REQ"),
            PDUType::ScanRsp => write!(f, "SCAN_RSP"),
            PDUType::ConnectReq => write!(f, "CONNECT_REQ"),
            PDUType::AdvScanInd => write!(f, "ADV_SCAN_IND"),
            PDUType::Unknown(x) => write!(f, "Unknown(0x{:x})", x),
        }
    }
}

impl core::fmt::Display for Advertisement {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(
            f,
            "type={:<20} len={}\taddr={}",
            format!("{}", self.pdu_type),
            self.length,
            self.address
        )
    }
}

impl core::fmt::Display for BluetoothPacket {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            BluetoothPacket::Advertisement(adv) => write!(f, "{}", adv),
            BluetoothPacket::Unimplemented(other) => write!(f, "Unimplemented({:x})", other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // use libbtbb_sys::*;

    /*
    #[test]
    fn test_find_lap_offset() {
        let lap_bits: Vec<u8> = vec![
            1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0,
            0, 1, 0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 0,
            0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1,
            1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0,
            1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1,
            0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 0, 1, 1, 0, 0, 0, 1, 1,
            1, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0,
            0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1,
            0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 0, 1,
            0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0, 0,
            1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 0, 1,
            1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 1,
        ];

        let mut btbb_packet: *mut btbb_packet = std::ptr::null_mut();

        unsafe {
            let ret = btbb_find_ac(
                lap_bits.as_ptr() as _,
                lap_bits.len() as _,
                LAP_ANY,
                1,
                (&mut btbb_packet) as *mut *mut btbb_packet,
            );

            assert!(ret < 0);
        }
    }
    */
}

use ice9_bindings::*;

use nom::{bytes::complete::take, number::complete::le_u32, IResult};

// TODO: いい感じに実装する
pub struct Bluetooth {
    #[allow(unused)]
    pub bits: Vec<u8>,

    #[allow(unused)]
    pub bytes: Vec<u8>,

    #[allow(unused)]
    pub packet: BluetoothPacket,

    #[allow(unused)]
    pub remain: Vec<u8>,
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
    pub fn from_bits(bits: &Vec<u8>, freq: usize) -> Result<Self, DecodeError> {
        let lap = unsafe { btbb_find_ac(bits.as_ptr() as _, bits.len() as _, 1) };

        if lap != 0xffffffff {
            return Err(DecodeError::FoundClassic(lap));
        }

        let p = unsafe { ble_easy(bits.as_ptr() as _, bits.len() as _, freq as _) };

        if p.is_null() {
            return Err(DecodeError::PacketNotFound);
        }

        let len = unsafe { (*p).len as usize };
        let slice = unsafe { (*p).data.as_slice(len) };

        let (remain, packet) = BluetoothPacket::from_bytes(&slice).unwrap();
        // FIXME: unwrap will panic if slice is too short

        Ok(Self {
            bits: bits.clone(),
            bytes: slice.to_vec(),
            packet,
            remain: remain.to_vec(),
        })
    }

    pub fn from_packet(packet: &_packet_t, freq: u32) -> Result<Self, DecodeError> {
        let slice = unsafe { std::slice::from_raw_parts(packet.bits, packet.bits_len as usize) };

        Self::from_bits(&slice.to_vec(), freq as usize)
    }
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

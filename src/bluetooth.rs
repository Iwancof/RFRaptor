// use ice9_bindings::*;

use nom::{bytes::complete::take, number::complete::le_u32, IResult};

use crate::bitops::BytePacket;

// TODO: いい感じに実装する
#[derive(Clone)]
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

#[derive(Clone)]
pub struct BluetoothPacket {
    pub inner: PacketInner,

    #[allow(unused)]
    pub crc: [u8; 3],
}

#[derive(Clone)]
pub enum PacketInner {
    Advertisement(Advertisement),
    Unimplemented(u32),
}

#[derive(Clone)]
pub struct Advertisement {
    pub pdu_header: PDUHeader,
    pub length: u8,
    pub address: MacAddress,
    pub data: Vec<AdvData>,
}

#[derive(Clone)]
pub struct MacAddress {
    pub address: [u8; 6],
}

#[derive(Clone)]
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

#[derive(Clone)]
pub struct PDUHeader {
    pdu_type: PDUType,
    rfu: bool,
    ch_sel: bool,
    tx_add: bool,
    rx_add: bool,
}

#[derive(Clone)]
pub struct AdvData {
    len: u8,
    data: Vec<u8>,
}

impl Bluetooth {
    pub fn from_bytes(mut byte_packet: BytePacket, freq: usize) -> Result<Self, DecodeError> {
        let len = byte_packet.bytes.len();
        let mut crc = [0, 0, 0];
        for (i, b) in byte_packet.bytes.drain(len - 3..).enumerate() {
            crc[i] = b;
        }

        // println!("crc: {:02x}{:02x}{:02x}", crc[0], crc[1], crc[2]);
        let (remain, packet_inner) = PacketInner::from_bytes(byte_packet.bytes.as_ref()).unwrap();
        // FIXME: unwrap will panic if slice is too short

        Ok(Self {
            bytes_packet: byte_packet.clone(),
            packet: BluetoothPacket {
                inner: packet_inner,
                crc,
            },
            remain: remain.to_vec(),
            freq,
        })
    }
}

impl PDUHeader {
    pub fn from_byte(mut byte: u8) -> Option<Self> {
        let pdu_type = match byte & 0b1111 {
            0b0000 => Some(PDUType::AdvInd),
            0b0001 => Some(PDUType::AdvDirectInd),
            0b0010 => Some(PDUType::AdvNonconnInd),
            0b0011 => Some(PDUType::ScanReq),
            0b0100 => Some(PDUType::ScanRsp),
            0b0101 => Some(PDUType::ConnectReq),
            0b0110 => Some(PDUType::AdvScanInd),
            x => Some(PDUType::Unknown(x)),
        };

        byte >>= 4;
        let rfu = byte & 0b1 == 1;

        byte >>= 1;
        let ch_sel = byte & 0b1 == 1;

        byte >>= 1;
        let tx_add = byte & 0b1 == 1;

        byte >>= 1;
        let rx_add = byte & 0b1 == 1;

        Some(PDUHeader {
            pdu_type: pdu_type?,
            rfu,
            ch_sel,
            tx_add,
            rx_add,
        })
    }
}

impl PacketInner {
    fn from_bytes(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, access_address) = le_u32(input)?;

        match access_address {
            0x8E89BED6 => {
                let (input, adv) = Advertisement::from_bytes(input)?;
                Ok((input, PacketInner::Advertisement(adv)))
            }
            other => Ok((input, PacketInner::Unimplemented(other))),
        }
    }
}

impl Advertisement {
    fn from_bytes(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, pdu_type) = take(1u8)(input)?;
        let pdu_type = PDUHeader::from_byte(pdu_type[0]).unwrap();

        let (input, length) = take(1u8)(input)?;
        let length = length[0];

        let (input, address) = MacAddress::from_bytes(input)?;

        let mut data = Vec::new();
        let mut input = input;

        while let Ok((remain, adv_data)) = AdvData::from_bytes(input) {
            data.push(adv_data);
            input = remain;
        }

        Ok((
            input,
            Advertisement {
                pdu_header: pdu_type,
                length,
                address,
                data,
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

impl AdvData {
    fn from_bytes(input: &[u8]) -> IResult<&[u8], Self> {
        let (input, len) = take(1u8)(input)?;
        let len = len[0];

        let (input, data) = take(len)(input)?;

        Ok((
            input,
            AdvData {
                len,
                data: data.to_vec(),
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

impl core::fmt::Display for PDUHeader {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self.pdu_type {
            PDUType::AdvInd => write!(f, "ADV_IND"),
            PDUType::AdvDirectInd => write!(f, "ADV_DIRECT_IND"),
            PDUType::AdvNonconnInd => write!(f, "ADV_NONCONN_IND"),
            PDUType::ScanReq => write!(f, "SCAN_REQ"),
            PDUType::ScanRsp => write!(f, "SCAN_RSP"),
            PDUType::ConnectReq => write!(f, "CONNECT_REQ"),
            PDUType::AdvScanInd => write!(f, "ADV_SCAN_IND"),
            PDUType::Unknown(x) => write!(f, "Unknown(0x{:x})", x),
        }?;

        write!(f, "[")?;

        let mut is_first = true;

        if self.rfu {
            if !is_first {
                write!(f, "|")?;
            }
            write!(f, "RFU")?;
            is_first = false;
        }

        if self.ch_sel {
            if !is_first {
                write!(f, "|")?;
            }
            write!(f, "CH_SEL")?;
            is_first = false;
        }

        if self.tx_add {
            if !is_first {
                write!(f, "|")?;
            }
            write!(f, "TX_ADD")?;
            is_first = false;
        }

        if self.rx_add {
            if !is_first {
                write!(f, "|")?;
            }
            write!(f, "RX_ADD")?;
        }

        write!(f, "]")
    }
}

impl core::fmt::Display for Advertisement {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        writeln!(
            f,
            "header={:<30} len={}\taddr={}",
            format!("{}", self.pdu_header),
            self.length,
            self.address,
        )?;

        for adv_data in &self.data {
            writeln!(f, "{}", adv_data)?;
        }

        Ok(())
    }
}

impl core::fmt::Display for PacketInner {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            PacketInner::Advertisement(adv) => write!(f, "{}", adv),
            PacketInner::Unimplemented(other) => write!(f, "Unimplemented({:x})", other),
        }
    }
}

impl core::fmt::Display for AdvData {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "len={} data={:02x?}", self.len, self.data)
    }
}

#[cfg(test)]
mod tests {
    // use libbtbb_sys::*;

    /*
    use super::*;

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

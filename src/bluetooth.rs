use ice9_bindings::*;

// TODO: いい感じに実装する
pub struct Bluetooth {
    #[allow(unused)]
    pub bits: Vec<u8>,

    #[allow(unused)]
    pub bytes: Vec<u8>,
    // TODO: other fields
}

pub enum DecodeError {
    #[allow(unused)]
    FoundClassic(u32),

    #[allow(unused)]
    PacketNotFound,
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

        Ok(Self {
            bits: bits.clone(),
            bytes: slice.to_vec(),
        })
    }

    pub fn from_packet(packet: &_packet_t, freq: u32) -> Result<Self, DecodeError> {
        let slice = unsafe { std::slice::from_raw_parts(packet.bits, packet.bits_len as usize) };

        Self::from_bits(&slice.to_vec(), freq as usize)
    }
}

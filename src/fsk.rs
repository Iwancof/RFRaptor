use ice9_bindings::*;

use num_complex::Complex;

pub struct FskDemod {
    fsk: ice9_bindings::fsk_demod_t,
}

impl FskDemod {
    pub fn new() -> Self {
        let mut fsk = core::mem::MaybeUninit::uninit();
        unsafe {
            // SAFETY: fsk is a valid MaybeUninit
            fsk_demod_init(fsk.as_mut_ptr());
        }

        let fsk = unsafe { fsk.assume_init() };

        Self { fsk }
    }

    pub fn demod(&mut self, data: &[Complex<f32>]) -> Option<_packet_t> {
        let mut packet = core::mem::MaybeUninit::zeroed();

        unsafe {
            fsk_demod(&mut self.fsk, data.as_ptr() as _, data.len() as _, packet.as_mut_ptr());
        }

        let packet = unsafe { packet.assume_init() };

        if !packet.demod.is_null() && !packet.bits.is_null() {
            Some(packet)
        } else {
            None
        }
    }
}


use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

use num_complex::Complex;

use crate::liquid::{liquid_do_int, liquid_get_pointer};

#[derive(Debug)]
struct Agc {
    crcf_s: std::ptr::NonNull<liquid_dsp_sys::agc_crcf_s>,
}

impl Agc {
    pub fn new() -> Self {
        use liquid_dsp_sys::*;
        let crcf = unsafe {
            let obj = liquid_get_pointer(|| agc_crcf_create()).expect("agc_crcf_create");
            liquid_do_int(|| agc_crcf_set_bandwidth(obj.as_ptr(), 0.25))
                .expect("agc_crcf_set_bandwidth");
            liquid_do_int(|| agc_crcf_set_signal_level(obj.as_ptr(), 1e-3))
                .expect("agc_crcf_set_signal_level");

            liquid_do_int(|| agc_crcf_squelch_enable(obj.as_ptr()))
                .expect("agc_crcf_squelch_enable");
            liquid_do_int(|| agc_crcf_squelch_set_threshold(obj.as_ptr(), -45.))
                .expect("agc_crcf_squelch_set_threshold");
            liquid_do_int(|| agc_crcf_squelch_set_timeout(obj.as_ptr(), 100))
                .expect("agc_crcf_squelch_set_timeout");

            obj
        };

        Self { crcf_s: crcf }
    }

    fn crcf(&self) -> *mut liquid_dsp_sys::agc_crcf_s {
        self.crcf_s.as_ptr()
    }

    pub fn execute(&mut self, mut signal: Complex<f32>) -> (Complex<f32>, SquelchStatus) {
        use liquid_dsp_sys::*;

        liquid_do_int(|| unsafe { agc_crcf_execute(self.crcf(), signal, &mut signal) })
            .expect("agc_crcf_execute");

        (signal, self.status())
    }

    pub fn status(&self) -> SquelchStatus {
        SquelchStatus::from_i32(unsafe { liquid_dsp_sys::agc_crcf_squelch_get_status(self.crcf()) })
            .expect("agc_crcf_squelch_get_status")
    }
}

impl Drop for Agc {
    fn drop(&mut self) {
        liquid_do_int(|| unsafe { liquid_dsp_sys::agc_crcf_destroy(self.crcf()) }).expect("agc_crcf_destroy");
    }
}

#[derive(Debug)]
pub struct Burst {
    crcf: Agc,
    in_burst: bool,
    burst: Vec<Complex<f32>>,
}

#[derive(FromPrimitive, Clone, Copy, Debug)]
pub enum SquelchStatus {
    Unknown = liquid_dsp_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_UNKNOWN as _,
    Enabled = liquid_dsp_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_ENABLED as _,
    Rise = liquid_dsp_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_RISE as _,
    SignalHi = liquid_dsp_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_SIGNALHI as _,
    Fall = liquid_dsp_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_FALL as _,
    SignalLo = liquid_dsp_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_SIGNALLO as _,
    Timeout = liquid_dsp_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_TIMEOUT as _,
    Disabled = liquid_dsp_sys::agc_squelch_mode_LIQUID_AGC_SQUELCH_DISABLED as _,
}

use chrono::prelude::*;

#[derive(Debug)]
pub struct Packet<'a> {
    pub data: &'a mut Vec<Complex<f32>>,

    #[allow(unused)]
    pub timestamp: DateTime<Utc>,
}

impl Burst {
    pub fn new() -> Self {
        Self {
            crcf: Agc::new(),
            in_burst: false,
            burst: Vec::new(),
        }
    }

    #[allow(unused)]
    pub fn catcher(&mut self, signal: Complex<f32>) -> Option<Packet> {
        let (signal, status) = self.crcf.execute(signal);

        match status {
            SquelchStatus::Rise => {
                self.in_burst = true;
                self.burst.clear();
            }
            SquelchStatus::SignalHi => {
                self.burst.push(signal);
            }
            SquelchStatus::Timeout => {
                self.in_burst = false;

                return Some(Packet {
                    data: &mut self.burst,
                    timestamp: Utc::now(),
                });
            }
            _x => {
                // println!("other: {:?}", x);
            }
        }

        None
    }
}

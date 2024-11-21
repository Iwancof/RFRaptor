use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

use num_complex::Complex;

#[derive(Debug)]
struct Crcf {
    crcf: liquid_dsp_sys::agc_crcf,
}

impl Crcf {
    pub fn new() -> Self {
        use liquid_dsp_sys::*;
        let crcf = unsafe {
            let obj = agc_crcf_create();
            agc_crcf_set_bandwidth(obj, 0.25);
            agc_crcf_set_signal_level(obj, 1e-3);

            agc_crcf_squelch_enable(obj);
            agc_crcf_squelch_set_threshold(obj, -45.);
            agc_crcf_squelch_set_timeout(obj, 100);
            obj
        };

        Self { crcf }
    }
    pub fn execute(&mut self, signal: Complex<f32>) -> (Complex<f32>, SquelchStatus) {
        use liquid_dsp_sys::*;

        let mut value = __BindgenComplex {
            re: signal.re,
            im: signal.im,
        };

        unsafe { agc_crcf_execute(self.crcf as _, value, &mut value) };

        (Complex::new(value.re, value.im), self.status())
    }

    pub fn status(&self) -> SquelchStatus {
        SquelchStatus::from_i32(unsafe { liquid_dsp_sys::agc_crcf_squelch_get_status(self.crcf) })
            .unwrap()
    }
}

impl Drop for Crcf {
    fn drop(&mut self) {
        unsafe { liquid_dsp_sys::agc_crcf_destroy(self.crcf) };
    }
}

#[derive(Debug)]
pub struct Burst {
    crcf: Crcf,
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
            crcf: Crcf::new(),
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

unsafe impl Send for Burst {}

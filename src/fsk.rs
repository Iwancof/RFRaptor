use liquid_dsp_sys::{freqdem, freqdem_create, freqdem_destroy};

use num_complex::Complex;
use num_traits::Signed;

/// at least 64 symbols are needed to calculate the median
const MEDIAN_SYMBOLS: usize = 64usize;

/// FSK demodulator
#[derive(Debug)]
pub struct FskDemod {
    #[allow(unused)]
    freqdem: freqdem,

    /// number of samples per symbol
    #[allow(unused)]
    pub sample_per_symbol: usize,

    /// number of symbols needed to calculate the median
    #[allow(unused)]
    pub need_symbol: usize,

    /// limit of the frequency offset
    #[allow(unused)]
    pub max_freq_offset: f32,
}

/// FSK demodulated packet
#[derive(Debug)]
pub struct Packet {
    /// demodulated bits
    #[allow(unused)]
    pub bits: Vec<u8>,

    /// demodulated data
    #[allow(unused)]
    pub demod: Vec<f32>,

    /// CFO (Carrier Frequency Offset)
    #[allow(unused)]
    pub cfo: f32,

    /// frequency deviation
    #[allow(unused)]
    pub deviation: f32,
}

impl Drop for FskDemod {
    fn drop(&mut self) {
        unsafe {
            freqdem_destroy(self.freqdem);
        }
    }
}

impl FskDemod {
    /// Create a new FSK demodulator
    ///
    /// # Arguments
    /// * `sample_rate` [Hz] - The sample rate of the incoming data
    /// * `num_channels` - The number of channels to use
    pub fn new(sample_rate: f32, num_channels: usize) -> Self {
        let freqdem = unsafe { freqdem_create(0.8f32) };

        let sample_per_symbol = (sample_rate / (num_channels as f32) / 1e6f32 * 2.0) as usize;
        Self {
            freqdem,
            sample_per_symbol,
            need_symbol: MEDIAN_SYMBOLS,
            max_freq_offset: 0.4f32,
        }
    }

    // Number of samples needed to calculate the median
    fn median_size(&self) -> usize {
        self.sample_per_symbol * self.need_symbol
    }

    // Raw demodulation
    fn liquid_demod(&mut self, data: &[Complex<f32>]) -> Vec<f32> {
        use liquid_dsp_sys::*;

        let mut demod: Vec<f32> = Vec::with_capacity(data.len());

        unsafe {
            freqdem_reset(self.freqdem);

            freqdem_demodulate_block(
                self.freqdem,
                data.as_ptr() as *const Complex<f32> as *mut __BindgenComplex<f32>,
                data.len() as _,
                demod.as_mut_ptr(),
            );

            demod.set_len(data.len());
        }

        demod
    }

    /// Demodulate the data
    pub fn demod(&mut self, data: &[Complex<f32>]) -> Option<Packet> {
        // too short to demodulate
        if data.len() < 8 + self.median_size() {
            return None;
        }

        // demodulate the data
        let mut demod = self.liquid_demod(data);

        // get the CFO and deviation
        let (cfo, deviation) = self.correction(&demod)?;
        demod.iter_mut().for_each(|d| {
            *d -= cfo;
            *d /= deviation;
        });

        // prepare to calculate the EWMA
        if demod[0].abs() > 1.5 {
            demod[0] = 0.;
        }

        let mut ewma = 0.;
        let bits = demod
            .iter()
            // skip silence at the beginning
            .skip_while(|v| {
                const ALPHA: f32 = 0.8;
                ewma = ewma * (1. - ALPHA) + v.abs() * ALPHA;

                ewma <= 0.5
            })
            // each symbol has 2 samples (?)
            .step_by(2)
            .map(|v| if v > &0.0 { 1 } else { 0 })
            .collect::<Vec<u8>>();

        Some(Packet {
            bits,
            demod,
            cfo,
            deviation,
        })
    }

    // Calculate the CFO and deviation
    fn correction(&self, demod: &[f32]) -> Option<(f32, f32)> {
        let mut pos = Vec::new();
        let mut neg = Vec::new();

        for d in demod.iter().skip(8).take(self.median_size()) {
            // too large frequency offset
            if d.abs() > self.max_freq_offset {
                return None;
            }

            if d.is_positive() {
                pos.push(*d);
            } else {
                neg.push(*d);
            }
        }

        // the data is too skewed
        if pos.len() < self.need_symbol / 4 || neg.len() < self.need_symbol / 4 {
            return None;
        }

        // sort the data
        pos.sort_by(|a, b| a.partial_cmp(b).unwrap());
        neg.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // calculate the median excluding the outliers
        let median = (pos[pos.len() * 3 / 4] + neg[neg.len() / 4]) / 2.0;

        let cfo = median;
        let deviation = pos[pos.len() * 3 / 4] - median;

        Some((cfo, deviation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ice9_bindings::*;
    use rand::{Rng, SeedableRng};

    use std::ops::Range;

    extern "C" {
        static mut samp_rate: std::os::raw::c_float;
        static mut channels: std::os::raw::c_uint;
    }

    fn create_magic() -> fsk_demod_t {
        let mut magic = core::mem::MaybeUninit::uninit();
        unsafe {
            fsk_demod_init(magic.as_mut_ptr());
        }

        let magic = unsafe { magic.assume_init() };

        unsafe {
            samp_rate = 20e6;
            channels = 20;
        }

        magic
    }

    extern "C" {
        fn cfo_median(
            fsk: *mut fsk_demod_t,
            demod: *const f32,
            len: std::os::raw::c_uint,
            cfo: *mut f32,
            deviation: *mut f32,
        ) -> std::os::raw::c_int;
    }

    fn calc_median_ice9(range: Range<f32>, num: usize) -> Option<(f32, f32)> {
        let mut magic = create_magic();

        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);

        let mut demod = Vec::new();
        for _ in 0..num {
            demod.push(rng.gen_range(range.clone()));
        }

        let mut cfo = 0.;
        let mut deviation = 0.;

        assert!(8 + (20 / 20 * 2) * 64 < demod.len());
        let num = unsafe {
            cfo_median(
                &mut magic,
                demod.as_ptr(),
                demod.len() as _,
                &mut cfo,
                &mut deviation,
            )
        };

        if num == 0 {
            return None;
        }

        Some((cfo, deviation))
    }

    fn calc_median_rust(range: Range<f32>, num: usize) -> Option<(f32, f32)> {
        let fsk = FskDemod::new(20e6, 20);

        let mut rng = rand::rngs::SmallRng::seed_from_u64(0);

        let mut demod = Vec::new();
        for _ in 0..num {
            demod.push(rng.gen_range(range.clone()));
        }

        fsk.correction(&demod)
    }

    #[test]
    fn test_median_in_range() {
        let ice9 = calc_median_ice9(-0.3..0.3, 2000);
        let rust = calc_median_rust(-0.3..0.3, 2000);

        assert_eq!(ice9, rust);
    }

    #[test]
    fn test_median_out_of_range() {
        let ice9 = calc_median_ice9(-0.5..0.5, 2000);
        let rust = calc_median_rust(-0.5..0.5, 2000);

        assert_eq!(ice9, rust);
        assert!(ice9.is_none());
    }

    #[test]
    fn test_median_pos_few() {
        let ice9 = calc_median_ice9(-0.3..0.0, 2000);
        let rust = calc_median_rust(-0.3..0.0, 2000);

        assert_eq!(ice9, rust);
        assert!(ice9.is_none());
    }

    #[test]
    fn test_median_few() {
        let ice9 = calc_median_ice9(-0.3..0.3, 200);
        let rust = calc_median_rust(-0.3..0.3, 200);

        assert_eq!(ice9, rust);
    }

    #[test]
    fn test_median_val_boundary() {
        let ice9 = calc_median_ice9(-0.41..0.41, 2000);
        let rust = calc_median_rust(-0.41..0.41, 2000);

        assert_eq!(ice9, rust);
        assert!(ice9.is_none());
    }

    fn calc_demod_ice9(data: &[Complex<f32>]) -> Option<Vec<u8>> {
        let mut magic = create_magic();

        let mut packet = core::mem::MaybeUninit::uninit();
        unsafe {
            fsk_demod(
                &mut magic,
                data.as_ptr() as *mut _,
                data.len() as _,
                packet.as_mut_ptr(),
            )
        };

        let packet = unsafe { packet.assume_init() };

        if packet.demod.is_null() || packet.bits.is_null() {
            return None;
        }

        let v: Vec<u8> =
            unsafe { core::slice::from_raw_parts(packet.bits, packet.bits_len as _).to_vec() };

        Some(v)
    }

    fn calc_demod_rust(data: &[Complex<f32>]) -> Option<Vec<u8>> {
        let mut fsk = FskDemod::new(20e6, 20);

        Some(fsk.demod(data)?.bits)
    }

    #[test]
    fn test_demod() {
        let data = vec![
            Complex::new(1.368325, 0.834682),
            Complex::new(0.813017, -0.198863),
            Complex::new(0.802122, -0.511718),
            Complex::new(-0.182673, -1.008206),
            Complex::new(-0.174114, -0.457808),
            Complex::new(0.198052, -0.772522),
            Complex::new(0.320830, -0.694577),
            Complex::new(-0.431436, -1.082072),
            Complex::new(-0.306003, -0.729896),
            Complex::new(-0.031379, -0.875162),
            Complex::new(0.250131, -0.585311),
            Complex::new(-0.330640, -0.936482),
            Complex::new(-0.317933, -0.660298),
            Complex::new(-0.329323, -1.123944),
            Complex::new(0.097938, -0.551434),
            Complex::new(-0.291886, -0.831618),
            Complex::new(-0.457952, -0.460691),
            Complex::new(-0.312774, -1.071331),
            Complex::new(0.348117, -1.039632),
            Complex::new(-0.531851, -1.073541),
            Complex::new(-0.472877, -0.622132),
            Complex::new(-0.172754, -1.002832),
            Complex::new(0.703300, -0.585969),
            Complex::new(0.709688, -0.332768),
            Complex::new(1.199063, 0.259799),
            Complex::new(0.559053, -0.713295),
            Complex::new(0.443945, -0.766970),
            Complex::new(0.625368, -0.741823),
            Complex::new(1.198505, 0.070060),
            Complex::new(0.690654, -0.832135),
            Complex::new(0.778481, -0.812558),
            Complex::new(0.506367, -0.547728),
            Complex::new(0.848810, 0.351151),
            Complex::new(0.017448, 0.890382),
            Complex::new(0.198844, 1.346893),
            Complex::new(0.515276, 0.482880),
            Complex::new(1.013035, 0.509110),
            Complex::new(0.618378, 0.139453),
            Complex::new(0.115084, 1.346132),
            Complex::new(-1.103302, 0.586637),
            Complex::new(-0.898475, -0.129840),
            Complex::new(-0.868759, -0.690014),
            Complex::new(-0.134140, -0.595454),
            Complex::new(0.644053, -0.774718),
            Complex::new(0.863611, 0.273378),
            Complex::new(0.270248, 0.250991),
            Complex::new(1.017545, 1.113974),
            Complex::new(0.602681, 0.346950),
            Complex::new(1.144229, 0.473370),
            Complex::new(0.690642, 0.353171),
            Complex::new(0.650056, 1.005676),
            Complex::new(-0.812298, 0.857713),
            Complex::new(-0.517943, 0.843652),
            Complex::new(-0.365417, 0.687964),
            Complex::new(0.645774, 0.847089),
            Complex::new(0.799287, 0.137861),
            Complex::new(1.046975, 0.139865),
            Complex::new(0.643054, 0.161509),
            Complex::new(1.021075, 0.814049),
            Complex::new(0.943411, -0.098188),
            Complex::new(0.826426, -0.643300),
            Complex::new(-0.259002, -0.908690),
            Complex::new(-0.684292, 0.043653),
            Complex::new(-0.664238, -0.259947),
            Complex::new(-0.608585, 0.557487),
            Complex::new(-1.272037, -0.189038),
            Complex::new(-0.780309, -0.484889),
            Complex::new(-1.187053, -0.139809),
            Complex::new(-0.896141, 0.909730),
            Complex::new(-1.171505, -0.317109),
            Complex::new(-0.674385, -0.425574),
            Complex::new(0.235070, -1.302984),
            Complex::new(0.877225, -0.722415),
            Complex::new(0.582264, -0.075096),
            Complex::new(0.803180, 0.422207),
            Complex::new(0.854462, -0.613555),
            Complex::new(0.454146, -0.928466),
            Complex::new(-0.364175, -0.928235),
            Complex::new(-0.821117, 0.053067),
            Complex::new(-0.975811, 0.245191),
            Complex::new(-0.722881, 0.938082),
            Complex::new(-0.823747, -0.072868),
            Complex::new(-0.570960, 0.188996),
            Complex::new(-0.585460, 0.417216),
            Complex::new(0.283688, 0.940980),
            Complex::new(0.586704, 0.612591),
            Complex::new(1.114555, 0.020423),
            Complex::new(0.301247, -1.170706),
            Complex::new(0.115406, -0.800375),
            Complex::new(-0.574732, -0.696033),
            Complex::new(-1.116657, -0.203851),
            Complex::new(-0.628261, -0.754851),
            Complex::new(0.164753, -0.652962),
            Complex::new(-1.035263, -0.506960),
            Complex::new(-0.945991, 0.722916),
            Complex::new(-0.580964, 0.987663),
            Complex::new(0.065416, 0.856524),
            Complex::new(-0.702520, 0.282628),
            Complex::new(-0.875499, 0.155423),
            Complex::new(-0.991761, -0.841447),
            Complex::new(0.204954, -0.794197),
            Complex::new(0.611271, -0.473401),
            Complex::new(1.462628, 0.667472),
            Complex::new(0.897045, 0.299059),
            Complex::new(1.009945, 0.623005),
            Complex::new(0.906466, 0.153100),
            Complex::new(0.990800, -0.233576),
            Complex::new(0.397831, -1.214462),
            Complex::new(-0.018301, -1.127126),
            Complex::new(0.076636, -1.226619),
            Complex::new(0.600225, 0.024337),
            Complex::new(0.534215, 0.164771),
            Complex::new(0.224132, 0.963983),
            Complex::new(-0.401156, 0.918772),
            Complex::new(-0.582545, 0.553466),
            Complex::new(-0.583811, -0.601879),
            Complex::new(-0.077219, -0.659236),
            Complex::new(0.171188, -0.937561),
            Complex::new(0.277385, -0.582520),
            Complex::new(-0.154281, -1.269733),
            Complex::new(-0.329265, -0.533844),
            Complex::new(0.310902, -1.406286),
            Complex::new(0.696234, -1.124641),
            Complex::new(-0.760776, -1.111605),
            Complex::new(-0.463218, -0.767959),
            Complex::new(-0.305725, -1.608560),
            Complex::new(0.514763, -0.442914),
            Complex::new(0.462813, -0.053395),
            Complex::new(0.580155, 1.044100),
            Complex::new(-0.354620, 0.954573),
            Complex::new(-0.098102, 1.272462),
            Complex::new(0.330949, 0.420739),
            Complex::new(0.817445, 0.944349),
            Complex::new(-0.203912, 0.926042),
            Complex::new(-0.102667, 0.898338),
            Complex::new(0.304109, 0.347631),
            Complex::new(0.886239, 0.819019),
            Complex::new(-0.127549, 0.455182),
            Complex::new(-0.404530, 0.709513),
            Complex::new(-0.850173, 0.068272),
            Complex::new(-0.718268, 0.029677),
            Complex::new(-0.328096, -1.343852),
            Complex::new(-0.038012, -0.861680),
            Complex::new(-0.646237, -0.686039),
            Complex::new(-0.564441, 0.054025),
            Complex::new(-0.829368, 0.614312),
            Complex::new(-0.105948, 1.481129),
            Complex::new(0.895239, 0.459234),
            Complex::new(1.050032, -0.224977),
            Complex::new(-0.010567, -0.941150),
            Complex::new(0.192806, -0.578255),
            Complex::new(0.726159, -0.593615),
            Complex::new(1.079728, -0.255428),
            Complex::new(0.195875, -0.701560),
            Complex::new(-0.045007, -0.352180),
            Complex::new(-1.558903, -0.593353),
            Complex::new(-0.801723, 0.692882),
            Complex::new(-0.368599, 0.852199),
            Complex::new(0.526108, 1.045050),
            Complex::new(1.036207, 0.408702),
            Complex::new(0.963449, -0.433376),
            Complex::new(-0.055417, -1.298393),
            Complex::new(-0.086332, -0.566018),
            Complex::new(-0.870035, -0.584141),
            Complex::new(-0.701450, 0.050323),
            Complex::new(-0.664759, -0.308036),
            Complex::new(-0.589035, -0.267050),
            Complex::new(-0.971799, -0.590780),
            Complex::new(-1.177769, 0.139942),
            Complex::new(-1.029779, -0.715212),
            Complex::new(-1.061083, -0.556705),
            Complex::new(-1.234714, -0.143316),
            Complex::new(-0.628309, 0.917184),
            Complex::new(0.082744, 0.520808),
            Complex::new(0.800778, 0.391564),
            Complex::new(0.887751, -0.270886),
            Complex::new(0.818568, 0.306422),
            Complex::new(0.990718, 0.306042),
            Complex::new(1.087343, 1.184476),
            Complex::new(-0.034645, 0.271707),
            Complex::new(-0.642133, 0.312150),
            Complex::new(-0.740505, -0.407048),
            Complex::new(-0.717868, -0.304839),
            Complex::new(-1.120992, 0.011427),
            Complex::new(-1.004495, 0.830976),
            Complex::new(-0.957939, -0.276698),
            Complex::new(-0.884314, -0.144918),
            Complex::new(-0.778356, -0.011891),
            Complex::new(-0.081032, 1.012949),
            Complex::new(0.479220, 0.672514),
            Complex::new(0.817158, 0.840640),
            Complex::new(-0.256507, 0.303522),
            Complex::new(-0.324599, 1.044837),
            Complex::new(-1.458500, -0.063488),
            Complex::new(-0.504821, -0.302635),
            Complex::new(-0.270499, -1.058188),
            Complex::new(0.411312, -0.932965),
            Complex::new(-0.185118, -0.891926),
            Complex::new(-0.786181, 0.239316),
            Complex::new(-0.441700, 0.441840),
            Complex::new(-0.364559, 0.908038),
            Complex::new(-0.805086, 0.382803),
            Complex::new(-0.429660, -0.206752),
            Complex::new(-0.655230, -1.281409),
            Complex::new(-0.198972, -0.942305),
            Complex::new(-0.757749, -0.426344),
            Complex::new(-0.595939, -0.559880),
            Complex::new(-0.735267, -1.370639),
            Complex::new(0.069272, -1.004803),
            Complex::new(1.001322, -0.305956),
            Complex::new(1.055291, -0.108674),
            Complex::new(0.563440, -1.260959),
            Complex::new(-0.071888, -0.476114),
            Complex::new(-0.530724, -0.200162),
            Complex::new(-0.579161, 0.566014),
            Complex::new(-0.199649, 0.220311),
            Complex::new(-0.274506, 0.910074),
            Complex::new(-0.853916, 0.403475),
            Complex::new(-1.595960, 0.326967),
            Complex::new(-1.318262, -1.018099),
            Complex::new(-1.038698, -0.369883),
            Complex::new(-1.724414, -0.631793),
            Complex::new(-0.800100, 1.035155),
            Complex::new(-0.070477, 0.864308),
            Complex::new(0.850441, 1.034492),
            Complex::new(0.809959, -0.157286),
            Complex::new(0.503534, -0.519615),
            Complex::new(0.083091, -0.507896),
            Complex::new(0.157693, -0.524136),
            Complex::new(0.169004, -0.830269),
            Complex::new(0.774252, -0.089443),
            Complex::new(0.526564, -0.006630),
            Complex::new(0.406520, 0.999442),
            Complex::new(0.554521, -0.162301),
            Complex::new(0.579436, -0.707560),
            Complex::new(-0.263165, -1.157137),
            Complex::new(-0.366240, -0.170647),
            Complex::new(0.616997, -1.001164),
            Complex::new(0.733981, -0.965759),
            Complex::new(-0.187991, -1.154306),
            Complex::new(-0.401511, -1.023835),
            Complex::new(-0.348601, -1.174792),
            Complex::new(0.260232, -0.482890),
            Complex::new(0.091248, -1.122843),
            Complex::new(-0.735805, -0.513331),
            Complex::new(-1.080449, -0.274635),
            Complex::new(-0.054798, 0.653977),
            Complex::new(0.105237, 0.627385),
            Complex::new(0.941342, 0.594805),
            Complex::new(0.949232, -0.289271),
            Complex::new(1.078820, -0.303811),
            Complex::new(0.559301, -0.797076),
            Complex::new(1.253145, 0.767761),
            Complex::new(0.435248, 0.513273),
            Complex::new(0.657957, 1.073672),
            Complex::new(0.520309, 0.526499),
            Complex::new(0.959287, 0.324887),
            Complex::new(0.512713, -0.912287),
            Complex::new(0.306466, -0.458641),
            Complex::new(-0.409012, -0.896318),
            Complex::new(-0.591558, 0.008508),
            Complex::new(-0.637984, 0.389161),
            Complex::new(-0.083185, 1.232184),
            Complex::new(-0.683117, 0.175660),
            Complex::new(-0.909857, 0.108207),
            Complex::new(-0.685238, -1.002361),
            Complex::new(0.648505, -0.912942),
            Complex::new(0.986470, -1.337391),
            Complex::new(1.151850, 0.169904),
            Complex::new(0.582645, 0.619849),
            Complex::new(0.614327, 1.048658),
            Complex::new(0.590850, 0.172720),
            Complex::new(0.943698, 0.374332),
            Complex::new(0.379386, 0.420166),
            Complex::new(0.750072, 0.712338),
            Complex::new(0.495011, 0.013157),
            Complex::new(0.636448, -0.364010),
            Complex::new(-0.328106, -1.173722),
            Complex::new(-0.055445, -0.482556),
            Complex::new(0.734016, -0.614642),
            Complex::new(1.195823, 0.313602),
            Complex::new(0.797856, 0.218668),
            Complex::new(0.839929, 0.583143),
            Complex::new(0.513281, -0.561762),
            Complex::new(1.050748, -0.040812),
            Complex::new(1.025125, 0.124072),
            Complex::new(0.476684, 1.329941),
            Complex::new(-0.465495, 1.051596),
            Complex::new(-0.292705, 1.057525),
            Complex::new(0.134570, 0.753579),
            Complex::new(0.856148, 0.703862),
            Complex::new(1.242334, 0.001117),
            Complex::new(0.687663, -0.788632),
            Complex::new(-0.280102, -1.103995),
            Complex::new(-0.398058, -0.175407),
            Complex::new(-1.148571, -0.045589),
            Complex::new(-0.712679, 0.769793),
            Complex::new(-1.316882, -0.182074),
            Complex::new(-0.512768, -0.611661),
            Complex::new(0.222466, -1.099739),
            Complex::new(0.606701, -0.471290),
            Complex::new(-0.368177, -0.562562),
            Complex::new(-0.549700, -0.794596),
            Complex::new(-0.645237, -1.050378),
            Complex::new(0.112678, -0.064716),
            Complex::new(-0.951612, -0.950660),
            Complex::new(-1.119069, -0.195210),
            Complex::new(-0.905352, 0.366967),
            Complex::new(-0.846372, 1.164659),
            Complex::new(-1.182884, 0.204485),
            Complex::new(-1.309711, 0.441215),
            Complex::new(-0.650532, 0.448327),
            Complex::new(0.094339, 1.005151),
            Complex::new(0.307098, 0.491640),
            Complex::new(0.544827, 0.820403),
            Complex::new(0.109298, 0.833198),
            Complex::new(0.174889, 1.325977),
            Complex::new(0.686372, 0.719607),
            Complex::new(0.965105, 0.578875),
            Complex::new(0.449033, 0.516505),
            Complex::new(0.373035, 1.059839),
            Complex::new(0.559798, 0.264407),
            Complex::new(0.977280, -0.196574),
            Complex::new(0.311118, -0.692193),
            Complex::new(0.414262, -0.715149),
            Complex::new(0.759620, -0.794328),
            Complex::new(0.895429, 0.181538),
            Complex::new(0.408027, 0.410333),
            Complex::new(0.304763, 0.949370),
            Complex::new(-0.849364, -0.064857),
            Complex::new(-1.239326, 0.069182),
            Complex::new(-1.032218, -0.574397),
            Complex::new(-0.267731, -0.357217),
            Complex::new(-0.793165, -0.476977),
            Complex::new(-0.939926, 0.907383),
            Complex::new(-0.258510, 0.938577),
            Complex::new(0.204782, 1.545631),
            Complex::new(-0.381411, 0.416979),
            Complex::new(-0.802351, 0.190497),
            Complex::new(-0.976268, -0.960637),
            Complex::new(-0.436057, -0.633116),
            Complex::new(-0.624971, -0.650865),
            Complex::new(-0.341637, 0.681291),
            Complex::new(0.225770, 0.735759),
            Complex::new(0.394892, 1.275234),
            Complex::new(-0.503036, 0.192048),
            Complex::new(-0.696894, 1.051401),
            Complex::new(-0.000393, 0.903713),
            Complex::new(0.600316, 1.505949),
            Complex::new(0.117888, 0.423247),
            Complex::new(-0.598872, 0.468886),
            Complex::new(-1.204861, -0.623961),
            Complex::new(-0.599812, -0.695548),
            Complex::new(0.287012, -1.225799),
            Complex::new(0.679635, -0.841447),
            Complex::new(-0.296123, -1.119900),
            Complex::new(-0.488280, -0.612373),
            Complex::new(-0.040089, -0.896284),
            Complex::new(0.793564, -0.194129),
            Complex::new(1.040026, -0.008214),
            Complex::new(0.859076, 0.278646),
            Complex::new(0.768513, -0.495084),
            Complex::new(0.765104, -0.178541),
            Complex::new(0.597859, -0.085387),
            Complex::new(0.961858, 0.683075),
            Complex::new(0.713256, -0.642410),
            Complex::new(0.601651, -0.621733),
            Complex::new(0.510268, -0.264600),
            Complex::new(1.170819, 0.515625),
            Complex::new(0.961710, -0.526732),
            Complex::new(0.239563, -0.948174),
            Complex::new(-1.057130, -0.749238),
            Complex::new(-0.582416, 0.347860),
            Complex::new(-0.477873, 0.542040),
            Complex::new(-0.524240, 0.889051),
            Complex::new(-0.958054, 0.226751),
            Complex::new(-1.143353, 0.371379),
            Complex::new(-0.786911, 0.394602),
            Complex::new(-0.291308, 1.162276),
            Complex::new(-1.071949, 0.041481),
            Complex::new(-0.951051, 0.575569),
            Complex::new(-0.584334, 0.609667),
            Complex::new(-0.349457, 1.162429),
            Complex::new(-0.938501, 0.222859),
            Complex::new(-0.479828, 0.722790),
            Complex::new(-0.356659, 0.564895),
            Complex::new(0.389776, 0.978087),
            Complex::new(0.728489, 0.359878),
            Complex::new(1.020904, -0.033611),
            Complex::new(0.124666, -1.301897),
            Complex::new(-0.081722, -0.185210),
            Complex::new(0.598160, -0.280936),
            Complex::new(1.063519, -0.206559),
            Complex::new(0.166453, -0.881481),
            Complex::new(0.364312, -0.677163),
            Complex::new(0.738631, -1.383859),
            Complex::new(0.787124, -0.565811),
            Complex::new(-0.041486, -1.085259),
            Complex::new(0.429600, -1.296716),
            Complex::new(0.395620, -1.077266),
            Complex::new(0.480276, 0.202178),
            Complex::new(0.559496, 0.139392),
            Complex::new(0.371213, 1.141039),
            Complex::new(-0.628107, 1.083971),
            Complex::new(0.191625, 1.201171),
            Complex::new(0.139604, 0.509062),
            Complex::new(1.063021, 0.508055),
            Complex::new(1.129091, -0.601147),
            Complex::new(0.987046, -0.842043),
            Complex::new(-0.438050, -1.206532),
            Complex::new(-0.689788, -0.184527),
            Complex::new(-1.026835, 0.230740),
            Complex::new(-0.157927, 1.095943),
            Complex::new(0.141976, 0.939659),
            Complex::new(0.661462, 0.986093),
            Complex::new(-0.257717, 0.268596),
            Complex::new(-0.256498, 0.735022),
            Complex::new(0.079076, 0.494772),
            Complex::new(0.471055, 0.871724),
            Complex::new(-0.123484, 0.329963),
            Complex::new(-0.019820, 1.066940),
            Complex::new(0.418866, 0.557534),
            Complex::new(0.392035, 1.132598),
            Complex::new(-0.428755, 0.600705),
            Complex::new(-0.440374, 1.115737),
            Complex::new(0.189664, 0.798447),
            Complex::new(0.519351, 1.278615),
            Complex::new(-0.168192, 0.596292),
            Complex::new(-0.430258, 0.876546),
            Complex::new(0.400621, 0.742363),
            Complex::new(1.269100, 0.401210),
            Complex::new(0.855234, -0.838344),
            Complex::new(0.826660, -0.533569),
            Complex::new(0.914491, -0.891943),
            Complex::new(1.282373, -0.074819),
            Complex::new(0.932397, -0.785219),
            Complex::new(0.465923, -0.975178),
            Complex::new(-0.660243, -0.713344),
            Complex::new(-0.935779, 0.575288),
            Complex::new(-0.638597, 0.427230),
            Complex::new(0.312314, 1.034919),
            Complex::new(0.312496, 0.702239),
            Complex::new(0.944268, 0.178895),
            Complex::new(0.646211, -0.713618),
            Complex::new(0.394542, -0.731088),
            Complex::new(0.464022, -1.099718),
            Complex::new(0.763500, -0.746746),
            Complex::new(0.315161, -0.689542),
            Complex::new(0.242905, -0.876552),
            Complex::new(0.298120, -0.810356),
            Complex::new(0.718132, -0.327903),
            Complex::new(0.362316, -0.948305),
            Complex::new(0.164198, -1.029643),
            Complex::new(0.265566, -1.095659),
            Complex::new(0.901016, -0.491423),
            Complex::new(-0.078776, -1.031367),
            Complex::new(-0.250687, -0.362346),
            Complex::new(-0.956650, -0.322187),
            Complex::new(-0.412207, 0.782580),
            Complex::new(-0.239989, 0.650525),
            Complex::new(-0.291123, 0.966548),
            Complex::new(-0.411703, 0.724327),
            Complex::new(-0.597181, 1.164362),
            Complex::new(0.256968, 0.580118),
            Complex::new(0.336121, 1.416633),
            Complex::new(-0.469098, 0.929431),
            Complex::new(-1.013233, 0.745615),
            Complex::new(-1.266684, -0.169370),
            Complex::new(-0.464869, -0.090769),
            Complex::new(-1.347173, 0.000085),
            Complex::new(-0.332539, 0.871319),
            Complex::new(0.601487, 0.518513),
            Complex::new(0.538796, 0.596385),
            Complex::new(-0.349056, 0.530381),
            Complex::new(-0.382065, 1.050125),
            Complex::new(-1.182315, -0.412238),
            Complex::new(-1.163618, 0.303451),
            Complex::new(-0.769419, 0.451548),
            Complex::new(-0.644561, 0.688934),
            Complex::new(-0.781016, -0.432529),
            Complex::new(-0.813332, -0.442931),
            Complex::new(-0.464738, -1.113934),
            Complex::new(0.052894, -0.488960),
            Complex::new(-0.588923, -1.072596),
            Complex::new(-0.841023, -0.050627),
            Complex::new(-0.418697, 0.618053),
            Complex::new(-0.008879, 0.785845),
            Complex::new(-1.004068, -0.085747),
            Complex::new(-1.123434, -0.195793),
            Complex::new(-0.994435, 0.127499),
            Complex::new(-0.888406, 1.179657),
            Complex::new(-1.223317, 0.390769),
            Complex::new(-0.733665, -0.094295),
            Complex::new(-0.499594, -1.150270),
            Complex::new(-0.410097, -0.914800),
            Complex::new(-0.654446, -0.609691),
            Complex::new(-0.480535, 0.198540),
            Complex::new(-0.619846, -0.883573),
            Complex::new(0.361273, -0.272374),
            Complex::new(0.449966, -0.887363),
            Complex::new(0.879025, -0.578128),
            Complex::new(0.219832, -1.165756),
            Complex::new(-0.539804, -0.503258),
            Complex::new(-1.336543, -0.850296),
            Complex::new(-0.972139, 0.556185),
            Complex::new(-0.428525, 0.785972),
            Complex::new(0.011708, 1.055892),
            Complex::new(-0.442970, 0.501036),
            Complex::new(-0.714579, 0.916277),
            Complex::new(-0.056701, 0.948233),
            Complex::new(0.609914, 1.113203),
            Complex::new(1.141349, 0.021415),
            Complex::new(0.932502, -0.545850),
            Complex::new(-0.126809, -1.109293),
            Complex::new(-0.345556, -0.267798),
            Complex::new(-0.905561, -0.288274),
            Complex::new(-0.832298, 0.455296),
            Complex::new(-1.203311, -0.073920),
            Complex::new(-0.719701, 0.192070),
            Complex::new(-0.814838, 0.018079),
            Complex::new(-0.522662, 0.949283),
            Complex::new(0.325575, 0.782341),
            Complex::new(0.648941, 1.203715),
            Complex::new(-0.019620, 0.538093),
            Complex::new(-0.269458, 0.834776),
            Complex::new(-0.153123, 0.150269),
            Complex::new(0.262990, 1.283456),
            Complex::new(-0.185278, 0.957491),
            Complex::new(-0.419149, 0.719266),
            Complex::new(-0.819339, -0.315199),
            Complex::new(-0.402691, -0.684316),
            Complex::new(-0.313575, -1.204962),
            Complex::new(0.252097, -0.685457),
            Complex::new(-0.876636, -1.060433),
            Complex::new(-0.908868, -0.813497),
            Complex::new(0.024707, -1.135308),
            Complex::new(0.953181, -0.321139),
            Complex::new(0.748092, -0.160169),
            Complex::new(0.901661, 1.015274),
            Complex::new(-0.015046, 0.823218),
            Complex::new(-0.232589, 0.923352),
            Complex::new(-0.941437, 0.035292),
            Complex::new(-0.385312, -0.006650),
            Complex::new(-0.500608, 0.154794),
            Complex::new(-0.032051, 0.972050),
            Complex::new(0.259975, 0.803288),
            Complex::new(1.031380, 0.923354),
            Complex::new(0.045905, 0.665906),
            Complex::new(-0.361538, 1.142948),
            Complex::new(-1.134455, 0.456014),
            Complex::new(-0.533203, 0.002411),
            Complex::new(-0.654421, -0.827563),
            Complex::new(-0.245898, -0.739192),
            Complex::new(-0.746781, -0.590169),
            Complex::new(-0.954466, 0.234773),
            Complex::new(-0.736162, 0.465880),
            Complex::new(-0.118718, 1.049463),
            Complex::new(-0.837401, 0.524047),
            Complex::new(-0.918752, 0.591915),
            Complex::new(-0.660886, 0.712263),
            Complex::new(-0.177879, 1.434941),
            Complex::new(-0.979758, 0.519664),
            Complex::new(-1.158629, 0.178753),
            Complex::new(-0.960675, -0.589975),
            Complex::new(-0.068597, -0.708694),
            Complex::new(0.577034, -0.666946),
            Complex::new(1.138696, 0.381220),
            Complex::new(0.637154, 0.450716),
            Complex::new(0.442699, 0.946798),
            Complex::new(0.511341, 0.110936),
            Complex::new(1.046649, -0.042375),
            Complex::new(0.167648, -0.785020),
            Complex::new(-0.197638, -0.625320),
            Complex::new(0.183914, -0.954032),
            Complex::new(0.972866, -0.219289),
            Complex::new(1.107010, 0.227919),
            Complex::new(0.932352, 1.101853),
            Complex::new(0.640157, -0.278235),
            Complex::new(0.819717, -0.613225),
            Complex::new(0.116691, -1.177352),
            Complex::new(-0.085833, -0.726193),
            Complex::new(0.264015, -1.223862),
            Complex::new(0.663763, -0.681278),
            Complex::new(-0.272710, -1.029424),
            Complex::new(-0.646698, -0.218281),
            Complex::new(-1.264235, 0.055477),
            Complex::new(-0.582486, 0.840821),
            Complex::new(-0.167193, 0.891391),
            Complex::new(0.580683, 1.117458),
            Complex::new(-0.012755, 0.550061),
            Complex::new(-0.359239, 1.169949),
            Complex::new(0.444324, 0.558231),
            Complex::new(1.156050, 0.740756),
            Complex::new(0.903150, -0.421573),
            Complex::new(0.921592, -0.543887),
            Complex::new(0.905887, -0.312147),
            Complex::new(1.017358, 0.176877),
            Complex::new(0.643089, -0.556997),
            Complex::new(0.573195, -0.301114),
            Complex::new(0.565234, -0.389297),
            Complex::new(0.753029, 0.637333),
            Complex::new(-0.219102, 0.752966),
            Complex::new(-0.598729, 0.846856),
            Complex::new(-1.102099, 0.152388),
            Complex::new(-0.579333, -0.367489),
            Complex::new(-0.347014, -1.174452),
            Complex::new(-0.030034, -0.635333),
            Complex::new(-0.825659, -0.812026),
            Complex::new(-0.945350, -0.445872),
            Complex::new(-0.737563, -1.382437),
            Complex::new(0.093120, -1.131580),
            Complex::new(-0.845983, -0.860729),
            Complex::new(-0.736789, -0.473373),
            Complex::new(-0.753781, -1.042457),
            Complex::new(-0.206103, -0.471268),
            Complex::new(-0.732715, -0.629239),
            Complex::new(-0.686939, -0.167322),
            Complex::new(-0.649451, -0.843253),
            Complex::new(-0.299363, -0.741384),
            Complex::new(-0.989406, -0.675005),
            Complex::new(-0.991162, 0.264677),
            Complex::new(-0.447840, 0.696992),
            Complex::new(0.561237, 1.134223),
            Complex::new(0.737094, 0.061038),
            Complex::new(1.008175, 0.383467),
            Complex::new(0.268718, 0.595399),
            Complex::new(0.016407, 1.299299),
            Complex::new(-0.691831, 0.630235),
            Complex::new(-0.874192, 0.375347),
            Complex::new(-1.037429, -0.729924),
            Complex::new(0.030605, -0.829893),
            Complex::new(0.635891, -0.865838),
            Complex::new(1.126608, -0.043963),
            Complex::new(0.807082, 0.003730),
            Complex::new(0.676997, 0.755050),
            Complex::new(0.841321, 0.044402),
            Complex::new(1.032670, 0.085603),
            Complex::new(0.740810, 0.134259),
            Complex::new(0.854421, 0.839654),
            Complex::new(0.930439, -0.049210),
            Complex::new(1.326638, -0.113348),
            Complex::new(0.855364, -0.107738),
            Complex::new(0.687243, 1.100902),
            Complex::new(-0.119454, 0.599031),
            Complex::new(-0.517459, 1.082350),
            Complex::new(-0.093125, 0.587987),
            Complex::new(0.873527, 0.608981),
            Complex::new(0.853170, -0.415349),
            Complex::new(0.666764, -0.407323),
            Complex::new(0.681989, -0.009442),
            Complex::new(1.038960, 0.318676),
            Complex::new(0.720724, -0.597111),
            Complex::new(0.783266, -0.460462),
            Complex::new(0.931118, -0.398584),
            Complex::new(1.287117, 0.291180),
            Complex::new(0.624707, -0.352786),
            Complex::new(0.358795, -0.735542),
            Complex::new(-0.617991, -1.228942),
            Complex::new(-0.529947, -0.779972),
            Complex::new(-0.310428, -1.028038),
            Complex::new(0.603061, -0.466131),
            Complex::new(0.918279, -0.519606),
            Complex::new(1.082764, 0.753988),
            Complex::new(0.172768, 0.785520),
            Complex::new(-0.413984, 0.934969),
            Complex::new(-1.139855, 0.002443),
            Complex::new(-0.884120, -0.179296),
            Complex::new(-0.496585, -1.240110),
            Complex::new(0.423444, -0.726588),
            Complex::new(0.893069, -0.590285),
            Complex::new(1.037783, -0.100660),
            Complex::new(0.376019, -0.929428),
            Complex::new(-0.107681, -0.795851),
            Complex::new(-0.793559, -0.793652),
            Complex::new(-0.675247, 0.131737),
            Complex::new(-0.664092, 0.495524),
            Complex::new(0.190599, 1.245576),
            Complex::new(0.429726, 0.392935),
            Complex::new(0.805621, 0.631822),
            Complex::new(0.607053, 0.570560),
            Complex::new(0.052833, 1.181362),
            Complex::new(-0.819994, 0.298047),
            Complex::new(-0.754199, -0.001256),
            Complex::new(-0.791922, -1.129576),
            Complex::new(0.114496, -0.731854),
            Complex::new(0.677189, -1.021673),
            Complex::new(0.728689, -0.388754),
            Complex::new(0.028189, -1.100476),
            Complex::new(-0.450465, -0.688757),
            Complex::new(-1.165029, -0.362231),
            Complex::new(-0.682730, 0.866827),
            Complex::new(-0.136381, 0.691489),
            Complex::new(0.715003, 0.902661),
            Complex::new(0.772798, -0.177940),
            Complex::new(0.980505, 0.106637),
            Complex::new(0.793047, 0.189411),
            Complex::new(0.246365, 1.124561),
            Complex::new(-0.637701, 0.712966),
            Complex::new(-0.617092, 0.846436),
            Complex::new(0.020009, 0.768404),
            Complex::new(0.726281, 0.853599),
            Complex::new(0.652690, -0.103391),
            Complex::new(0.797276, -0.000135),
            Complex::new(0.503504, -0.174155),
            Complex::new(0.590576, 0.670085),
            Complex::new(0.279025, -0.176208),
        ];

        let ice9 = calc_demod_ice9(&data);
        let rust = calc_demod_rust(&data);

        assert_eq!(ice9, rust);
        assert!(ice9.is_some());
    }
}

use ice9_bindings::{_pfbch2_t, pfbch2_execute};

use num_complex::Complex;

pub fn channelize(magic: &mut _pfbch2_t, input: &[Complex<i8>]) -> Vec<Complex<f32>> {
    assert_eq!(input.len(), 20 / 2);
    let mut output = Vec::with_capacity(20);

    // SAFETY: Complex<T> has `repr(C)` layout
    let flat_chunk = input.as_ptr() as *mut i8;
    let mut working = [0i16; 96 * 2];

    unsafe {
        pfbch2_execute(
            // &mut magic as _,
            magic as _,
            flat_chunk,
            working.as_mut_ptr() as *mut i16,
        );
    }

    working[..20 * 2].array_chunks::<2>().for_each(|[re, im]| {
        let re = *re as f32 / 32768.0;
        let im = *im as f32 / 32768.0;
        output.push(Complex::new(re, im));
    });

    output
}

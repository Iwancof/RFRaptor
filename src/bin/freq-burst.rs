use std::iter::once;

use hydro_strike::{burst, fsk};
use num_complex::Complex32;

fn main() {
    let mut fsk = fsk::FskMod::new(2427e6, 20);

    let mut data = vec![];
    for i in 1..20 {
        data.push(1);
        for _ in 0..i {
            data.push(0);
        }
    }

    let modulated = fsk
        .modulate(&data)
        .unwrap();

    let mut agc = burst::Burst::new();

    let mut got = None;

    for i in 0..100 {
        let gamma = 1e-3;
        let noise = Complex32::new(0., 2. * std::f32::consts::PI * 0.0193 * i as f32).exp();

        // let r = agc.crcf.execute(gamma * noise);
        // println!("{:?}", r);

        if let Some(packet) = agc.catcher(gamma * noise) {
            got = Some(packet.data.clone());
        }
    }

    for m in &modulated {
        // let r = agc.crcf.execute(*m);
        // println!("{:?}", r);
        if let Some(packet) = agc.catcher(*m) {
            got = Some(packet.data.clone());
        }
    }

    for i in 0..200 {
        let gamma = 1e-3;
        let noise = Complex32::new(0., 2. * std::f32::consts::PI * 0.0193 * i as f32).exp();

        // let r = agc.crcf.execute(gamma * noise);
        // println!("{:?}", r);

        if let Some(packet) = agc.catcher(gamma * noise) {
            got = Some(packet.data.clone());
        }
    }

    // println!("{:?}", got);

    println!("len: {}", got.as_ref().unwrap().len());

    let mut demod = fsk::FskDemod::new(20e6, 20);
    let d = demod.demodulate(&got.unwrap()).unwrap();

    println!("{:?}", d);
    println!("{}", d.bits.len());
}

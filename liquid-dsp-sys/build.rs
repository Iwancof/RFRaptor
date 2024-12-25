use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rutn-if-changed=liquid-dsp/libliquid.a");

    /*
    std::process::Command::new("sh")
        .args(&["-c", "cd liquid-dsp && ./bootstrap.sh && ./configure && make -j"])
        .status()
        .expect("Failed to build liquid-dsp");
    */

    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();

    let bindgen = bindgen::Builder::default()
        .generate_comments(true)
        .header(format!("{}/liquid-dsp/include/liquid.h", crate_dir));

    let bindgen = if env::var("CARGO_FEATURE_NUM_COMPLEX").is_ok() {
        // replace complex types with num-complex from bindgen
        bindgen
            .blocklist_type("liquid_float_complex")
            .blocklist_type("liquid_double_complex")
    } else {
        bindgen
    };

    let bindgen = bindgen.generate().expect("Unable to generate bindings");

    bindgen
        .write_to_file(format!("{}/liquid.rs", out_dir))
        .expect("Couldn't write bindings!");

    // add library search path
    println!("cargo:rustc-link-search=native={}/liquid-dsp", crate_dir);

    println!("cargo:rustc-link-lib=dylib=liquid");
    println!("cargo:rustc-link-lib=dylib=fftw3f");
}

fn main() {
    let dst = cmake::Config::new("libbtbb")
        .define("ENABLE_PYTHON", "OFF")
        .build();

    println!("cargo::rustc-link-search={}/lib/", dst.display());
    println!("cargo::rustc-link-lib=dylib=btbb");

    let mut bindings = bindgen::Builder::default();
    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    for entry in std::fs::read_dir("./libbtbb/lib/src/").unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().unwrap() == "h" {
            bindings = bindings.header(path.to_str().unwrap());
        }
    }

    // bindings = bindings.header("/usr/local/include/btbb.h");

    bindings
        .generate_comments(true)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

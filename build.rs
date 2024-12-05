fn main() {
    println!("cargo:rerun-if-changed=src/apply_filter.c");

    use cc::Build;

    // use AVX2
    Build::new()
        .file("src/apply_filter.c")
        .opt_level(2)
        .flag("-mavx2")
        .flag("-march=native")
        .define("FORTIFY_SOURCE", "2")
        .warnings(true)
        .extra_warnings(true)
        .compile("libapply_filter.a");

}

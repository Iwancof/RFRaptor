use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    // build the C++ code (cmake)

    let projects = [
        "SoapyHackRF",
        "soapy-utils/soapy-file",
        "soapy-utils/soapy-virtual",
    ];
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    // check release or debug
    let profile = std::env::var("PROFILE").unwrap();
    let build_type = if profile == "release" {
        "Release"
    } else {
        "Debug"
    };

    for project in projects.iter() {
        use cmake;

        let project_dir = format!("{}/{}", manifest_dir, project);

        println!("cargo::rerun-if-changed={}", project_dir);

        // let status = Command::new("cmake")
        //     .args(["-S", &project_dir, "-B", &build_dir, "-DCMAKE_BUILD_TYPE=", build_type])
        //     .status()
        //     .expect("Failed to run cmake");

        // if !status.success() {
        //     panic!("Failed to run cmake");
        // }

        // let status = Command::new("cmake")
        //     .args(["--build", &build_dir])
        //     .status()
        //     .expect("Failed to run cmake");

        // if !status.success() {
        //     panic!("Failed to run cmake");
        // }

        cmake::Config::new(&project_dir)
            .profile(build_type)
            .define("CMAKE_EXPORT_COMPILE_COMMANDS", "YES")
            .build();
    }
}

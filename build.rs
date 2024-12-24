use std::{process::Command};

fn main() {
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    // build the C++ code (cmake)

    let dest_dir = std::env::var("OUT_DIR").unwrap();
    let projects = ["SoapyHackRF", "soapy-utils/soapy-file", "soapy-utils/soapy-virtual"];
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    for project in projects.iter() {
        let project_dir = format!("{}/{}", manifest_dir, project);
        let build_dir = format!("{}/{}", dest_dir, project);

        println!("cargo:rerun-if-changed={}", project_dir);

        let status = Command::new("cmake")
            .args(&["-S", &project_dir, "-B", &build_dir])
            .status()
            .expect("Failed to run cmake");

        if !status.success() {
            panic!("Failed to run cmake");
        }

        let status = Command::new("cmake")
            .args(&["--build", &build_dir])
            .status()
            .expect("Failed to run cmake");
        
        if !status.success() {
            panic!("Failed to run cmake");
        }
    }
}

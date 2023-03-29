use std::{env, error::Error, process::Command};

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Get the name of the package.
    let kernel_name = env::var("CARGO_PKG_NAME")?;

    let status = Command::new("./deps.sh")
        .args(["makeimg"])
        .status()
        .unwrap();
    assert!(status.success());

    println!("cargo:rustc-link-arg-bin={kernel_name}=--script=.cargo/linker.ld");

    // Add linker args.
    println!("cargo:rustc-link-arg-bin={kernel_name}=--gc-sections");

    // Have cargo rerun this script if the linker script or CARGO_PKG_ENV changes.
    println!("cargo:rerun-if-changed=.cargo/linker.ld");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_NAME");
    println!("cargo:rerun-if-changed=build.rs");

    Ok(())
}

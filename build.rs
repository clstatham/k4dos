use std::{env, error::Error};

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Get the name of the package.
    let kernel_name = env::var("CARGO_PKG_NAME")?;

    // Tell rustc to pass the linker script to the linker.
    // println!("cargo:rustc-link-arg-bin={kernel_name}=--script=conf/linker.ld");

    println!("cargo:rustc-link-arg-bin={kernel_name}=--gc-sections");

    // Have cargo rerun this script if the linker script or CARGO_PKG_ENV changes.
    println!("cargo:rerun-if-changed=.cargo/linker.ld");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_NAME");

    Ok(())
}

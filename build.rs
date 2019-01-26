use std::{env, error::Error};

fn main() -> Result<(), Box<Error>> {
    let target = env::var("TARGET")?;

    match &*target {
        "thumbv7m-none-eabi" | "thumbv7em-none-eabi" | "thumbv7em-none-eabihf" => {
            println!("cargo:rustc-cfg=armv7m")
        }
        _ => {}
    }

    Ok(())
}

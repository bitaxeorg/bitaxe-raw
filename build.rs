fn main() {
    linker_be_nice();
    check_llvm_version();
    println!("cargo:rustc-link-arg=-Tdefmt.x");
    // make sure linkall.x is the last linker script (otherwise might cause problems with flip-link)
    println!("cargo:rustc-link-arg=-Tlinkall.x");
}

// esp toolchains built on LLVM 21 (Xtensa Rust 1.94.x / 1.95.x) fail to
// compile release builds for the Xtensa target: the backend can't select
// XtensaISD::PCREL_WRAPPER for a constant-pool string reference and aborts
// with `rustc-LLVM ERROR: Cannot select`. LLVM 20 (Xtensa Rust 1.93.0.0) is
// the last good version. Turn that cryptic crash into an actionable message.
// Revisit the version bound once esp-rs ships a fixed LLVM 21.
fn check_llvm_version() {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = match std::process::Command::new(&rustc)
        .args(["--version", "--verbose"])
        .output()
    {
        Ok(output) => output,
        // Can't probe rustc; don't block the build over it.
        Err(_) => return,
    };

    let major = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("LLVM version:"))
        .and_then(|version| version.trim().split('.').next())
        .and_then(|major| major.parse::<u32>().ok());

    if let Some(major) = major {
        if major >= 21 {
            eprintln!();
            eprintln!("This esp toolchain bundles LLVM {major}, which miscompiles optimized");
            eprintln!("Xtensa builds (XtensaISD::PCREL_WRAPPER cannot be selected). Install the");
            eprintln!("last known-good toolchain:");
            eprintln!();
            eprintln!("    espup install --toolchain-version 1.93.0.0");
            eprintln!();
            std::process::exit(1);
        }
    }
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                "_defmt_timestamp" => {
                    eprintln!();
                    eprintln!("💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`");
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}

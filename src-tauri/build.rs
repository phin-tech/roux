use std::env;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn ensure_ci_external_bin_for_host() -> std::io::Result<()> {
    // CI runs Rust checks on Linux where we don't ship a prebuilt
    // roux host binary in-repo. Tauri validates externalBin paths
    // during build script execution, so create a host-triple placeholder
    // only in CI when the expected file is missing.
    if env::var_os("CI").is_none() {
        return Ok(());
    }

    let triple =
        env::var("TAURI_ENV_TARGET_TRIPLE").or_else(|_| env::var("TARGET")).unwrap_or_default();
    if triple.is_empty() {
        return Ok(());
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()));
    let bin_dir = manifest_dir.join("binaries");
    fs::create_dir_all(&bin_dir)?;

    for name in ["roux", "roux-cli"] {
        let expected = if triple.contains("windows") {
            bin_dir.join(format!("{name}-{triple}.exe"))
        } else {
            bin_dir.join(format!("{name}-{triple}"))
        };
        if expected.exists() {
            continue;
        }

        if triple.contains("windows") {
            fs::write(
                &expected,
                format!("@echo off\r\necho {name} placeholder for CI checks 1>&2\r\nexit /b 1\r\n"),
            )?;
        } else {
            fs::write(
                &expected,
                format!(
                    "#!/usr/bin/env sh\nprintf '%s\\n' '{name} placeholder for CI checks' >&2\nexit 1\n"
                ),
            )?;
            #[cfg(unix)]
            fs::set_permissions(&expected, fs::Permissions::from_mode(0o755))?;
        }

        println!("cargo:warning=created CI placeholder externalBin at {}", expected.display());
    }
    Ok(())
}

fn main() {
    println!("cargo:rerun-if-env-changed=CI");
    println!("cargo:rerun-if-env-changed=TAURI_ENV_TARGET_TRIPLE");
    if let Err(e) = ensure_ci_external_bin_for_host() {
        println!("cargo:warning=failed to prepare CI externalBin placeholder: {e}");
    }
    tauri_build::build()
}

use std::env;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn ensure_ci_external_bin_for_host() -> std::io::Result<()> {
    // CI runs Rust checks on Linux where we don't ship a prebuilt
    // roux-cli host binary in-repo. Tauri validates externalBin paths
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
    let expected = if triple.contains("windows") {
        bin_dir.join(format!("roux-cli-{triple}.exe"))
    } else {
        bin_dir.join(format!("roux-cli-{triple}"))
    };
    if expected.exists() {
        return Ok(());
    }

    fs::create_dir_all(&bin_dir)?;

    if triple.contains("windows") {
        fs::write(
            &expected,
            "@echo off\r\necho roux-cli placeholder for CI checks 1>&2\r\nexit /b 1\r\n",
        )?;
    } else {
        fs::write(
            &expected,
            "#!/usr/bin/env sh\nprintf '%s\\n' 'roux-cli placeholder for CI checks' >&2\nexit 1\n",
        )?;
        #[cfg(unix)]
        fs::set_permissions(&expected, fs::Permissions::from_mode(0o755))?;
    }

    println!("cargo:warning=created CI placeholder externalBin at {}", expected.display());
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

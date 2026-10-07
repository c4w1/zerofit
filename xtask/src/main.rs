//! Repository automation, run as `cargo xtask <command>`.
//!
//! - `codegen [--check]`: regenerate `crates/zerofit-profile/src/generated/`
//!   from `crates/zerofit-profile/codegen/profile-subset.json`. With
//!   `--check`, fail if the checked-in files differ (used by CI).
//! - `extract`: rebuild `profile-subset.json` from the FIT SDK's
//!   `Profile.xlsx` (path in `FIT_PROFILE_XLSX`), then run `codegen`.
//! - `expected`: write `<name>.expected.json` for every fixture from Garmin's
//!   FitCSVTool (path to `FitCSVTool.jar` in `FIT_CSV_TOOL`; needs `java`).

// A developer tool: panics abort the tool, not a user's program.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

mod expected;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("codegen") => codegen(args.iter().any(|a| a == "--check")),
        Some("extract") => extract().and_then(|()| codegen(false)),
        Some("expected") => load_profile()
            .and_then(|p| expected::run(&root().join("crates/zerofit/tests/fixtures"), &p)),
        _ => Err("usage: cargo xtask <codegen [--check] | extract | expected>".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn subset_path() -> PathBuf {
    root().join("crates/zerofit-profile/codegen/profile-subset.json")
}

fn generated_dir() -> PathBuf {
    root().join("crates/zerofit-profile/src/generated")
}

fn extract() -> Result<(), String> {
    let xlsx = std::env::var_os("FIT_PROFILE_XLSX")
        .map(PathBuf::from)
        .ok_or("set FIT_PROFILE_XLSX to the FIT SDK's Profile.xlsx")?;
    let version = std::env::var("FIT_SDK_VERSION")
        .ok()
        .or_else(|| sdk_version_from_path(&xlsx));
    let version = version
        .ok_or("cannot infer the SDK version from the path; set FIT_SDK_VERSION (e.g. 21.171)")?;
    let profile = zerofit_codegen::extract(&xlsx, &version)?;
    let json = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())? + "\n";
    write(&subset_path(), &json)?;
    eprintln!("wrote {}", subset_path().display());
    Ok(())
}

/// `.../FitSDKRelease_21.171.00/Profile.xlsx` -> `21.171`.
fn sdk_version_from_path(xlsx: &Path) -> Option<String> {
    let dir = xlsx.parent()?.file_name()?.to_str()?;
    let version = dir.strip_prefix("FitSDKRelease_")?;
    let mut parts = version.split('.');
    Some(format!("{}.{}", parts.next()?, parts.next()?))
}

fn load_profile() -> Result<zerofit_codegen::Profile, String> {
    let json = std::fs::read_to_string(subset_path())
        .map_err(|e| format!("cannot read {}: {e}", subset_path().display()))?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

fn codegen(check: bool) -> Result<(), String> {
    let profile = load_profile()?;
    let files = zerofit_codegen::generate(&profile)?;
    let mut stale = Vec::new();
    for file in &files {
        let path = generated_dir().join(file.path);
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        // Compare ignoring line endings (Git may check files out with CRLF).
        if current.replace("\r\n", "\n") == file.contents {
            continue;
        }
        if check {
            stale.push(path.display().to_string());
        } else {
            write(&path, &file.contents)?;
            eprintln!("wrote {}", path.display());
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "generated code is out of date (run `cargo xtask codegen`):\n  {}",
            stale.join("\n  ")
        ))
    }
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, contents).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn does_not_depend_on_our_decoder() {
        let manifest = include_str!("../Cargo.toml");
        let deps = manifest.split("[dependencies]").nth(1).unwrap_or_default();
        for line in deps.lines() {
            let name = line.split('=').next().unwrap_or_default().trim();
            assert!(
                !name.starts_with("zerofit") || name == "zerofit-codegen",
                "xtask must not depend on {name}: expected values must come from FitCSVTool"
            );
        }
    }

    #[test]
    fn sdk_version_from_path() {
        let p = std::path::Path::new("/x/FitSDKRelease_21.171.00/Profile.xlsx");
        assert_eq!(super::sdk_version_from_path(p).as_deref(), Some("21.171"));
    }
}

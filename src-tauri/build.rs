use std::process::Command;

/// Stamp build-time provenance into the binary, so the About dialog can report
/// exactly which sources a build came from. Values fall back to "unknown" when
/// building outside a git checkout (e.g. from a source tarball).
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn main() {
    let commit = git(&["rev-parse", "--short=9", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let date = git(&["log", "-1", "--format=%cs"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=BC_COMMIT={commit}");
    println!("cargo:rustc-env=BC_COMMIT_DATE={date}");
    // Without this, a new commit would not rebuild the stamp.
    println!("cargo:rerun-if-changed=../.git/HEAD");

    tauri_build::build();
}

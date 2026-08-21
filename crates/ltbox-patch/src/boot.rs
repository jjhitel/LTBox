//! Boot image patching — wraps magiskboot for root operations.

use fs_err as fs;
use std::path::Path;

use ltbox_core::{LtboxError, Result};

/// Return the top-level ramdisk produced by unpacking boot or init_boot.
pub(crate) fn root_ramdisk_name(work_dir: &Path) -> Result<&'static str> {
    const RAMDISK: &str = "ramdisk.cpio";
    if work_dir.join(RAMDISK).is_file() {
        Ok(RAMDISK)
    } else {
        Err(LtboxError::Patch(
            "ramdisk.cpio not found after unpack".into(),
        ))
    }
}

/// Unpack a boot image into components. Non-zero magiskboot exit becomes `Err`.
pub fn unpack(image: &Path, work_dir: &Path) -> Result<()> {
    let name = image
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("boot.img");
    let dst = work_dir.join(name);
    if image != dst {
        fs::copy(image, &dst).map_err(|e| LtboxError::BootImage(e.to_string()))?;
    }
    let code = run_magiskboot(work_dir, &["unpack", name])?;
    if code == 0 {
        Ok(())
    } else {
        check_magiskboot("unpack", code)
    }
}

/// Repack boot image from components. Non-zero magiskboot exit becomes `Err`.
pub fn repack(orig_image: &str, work_dir: &Path) -> Result<()> {
    check_magiskboot("repack", run_magiskboot(work_dir, &["repack", orig_image])?)
}

/// CPIO operations on ramdisk. Raw exit code — caller decides what's an error.
/// Use [`cpio_checked`] for mutating commands where non-zero means failure.
/// Leave this untouched for `test` / `exists` whose rc is a status flag.
pub fn cpio(work_dir: &Path, cpio_file: &str, commands: &[&str]) -> Result<i32> {
    let mut args = vec!["cpio", cpio_file];
    args.extend_from_slice(commands);
    run_magiskboot(work_dir, &args)
}

/// CPIO operations that must succeed — non-zero magiskboot exit becomes `Err`.
/// Use for `add`, `mv`, `mkdir`, `backup`, `patch`, etc. where any failure
/// leaves the ramdisk half-patched and the repack unsafe to ship.
pub fn cpio_checked(work_dir: &Path, cpio_file: &str, commands: &[&str]) -> Result<()> {
    check_magiskboot(
        &format!("cpio {}", commands.join(" ")),
        cpio(work_dir, cpio_file, commands)?,
    )
}

/// CPIO operations with extra env vars set for the duration of the call.
///
/// Required for `cpio … patch` on Magisk/KernelSU/APatch flows: magiskboot's
/// patcher reads `KEEPVERITY` / `KEEPFORCEENCRYPT` from the process env at
/// call time. Without them magiskboot defaults to *stripping* dm-verity and
/// forceencrypt fstab flags — the opposite of what stock-preserving root
/// wants. Env is process-global; the CWD lock held by `run_magiskboot_with_env`
/// serializes the set/restore so concurrent calls don't leak values.
///
/// Always checked: patch is a mutation, a non-zero rc means nothing to repack.
pub fn cpio_with_env(
    work_dir: &Path,
    cpio_file: &str,
    commands: &[&str],
    envs: &[(&str, &str)],
) -> Result<()> {
    let mut args = vec!["cpio", cpio_file];
    args.extend_from_slice(commands);
    check_magiskboot(
        &format!("cpio {}", commands.join(" ")),
        run_magiskboot_with_env(work_dir, &args, envs)?,
    )
}

/// Map magiskboot exit code to a `Result`. Exit 0 = success; anything else
/// surfaces as `LtboxError::BootImage` with the operation label.
fn check_magiskboot(op: &str, code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(LtboxError::BootImage(format!(
            "magiskboot {op} failed (exit={code})"
        )))
    }
}

/// SHA1 hash of a file (computed in Rust, no magiskboot needed).
pub fn sha1(file_path: &Path) -> Result<String> {
    let data = fs::read(file_path).map_err(|e| LtboxError::BootImage(e.to_string()))?;
    Ok(sha1_hash(&data))
}

/// Compress a file. Non-zero magiskboot exit becomes `Err`.
pub fn compress(work_dir: &Path, format: &str, input: &str, output: &str) -> Result<()> {
    check_magiskboot(
        "compress",
        run_magiskboot(work_dir, &[&format!("compress={format}"), input, output])?,
    )
}

/// Detect and decompress a file. Non-zero magiskboot exit becomes `Err`.
pub fn decompress(work_dir: &Path, input: &str, output: &str) -> Result<()> {
    check_magiskboot(
        "decompress",
        run_magiskboot(work_dir, &["decompress", input, output])?,
    )
}

/// Process-wide CWD guard: `boot_main` resolves filenames relative to CWD,
/// so we must `chdir` into `work_dir`. PollDevice fires concurrently via
/// `spawn_blocking` — a static mutex serializes the chdir/run/restore sequence.
static MAGISKBOOT_CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run_magiskboot(work_dir: &Path, args: &[&str]) -> Result<i32> {
    run_magiskboot_with_env(work_dir, args, &[])
}

fn run_magiskboot_with_env(work_dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> Result<i32> {
    // Recover from poisoning: inner catch_unwind turns magiskboot panics
    // into errors, so the mutex stays safe to reuse.
    let _guard = MAGISKBOOT_CWD_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());

    let original_dir = std::env::current_dir().ok();
    std::env::set_current_dir(work_dir).map_err(|e| LtboxError::BootImage(e.to_string()))?;

    // Snapshot env values we're about to override so we can restore them.
    // Important because the env is process-global — leaking KEEPVERITY=true
    // into a subsequent call could invert behavior.
    let saved_envs: Vec<(&str, Option<String>)> = envs
        .iter()
        .map(|(k, _)| (*k, std::env::var(*k).ok()))
        .collect();
    for (k, v) in envs {
        // SAFETY: std::env::set_var is unsafe on Rust 1.82+ due to data races
        // in multithreaded programs. The CWD lock above serializes magiskboot
        // calls, but other threads might still read env concurrently. Callers
        // pass only static-config vars (KEEPVERITY etc.) that magiskboot
        // itself reads inside the same lock scope.
        unsafe { std::env::set_var(k, v) };
    }

    let mut full_args = vec!["magiskboot".to_string()];
    full_args.extend(args.iter().map(|s| s.to_string()));
    let cmds = magiskboot::base::CmdArgs::from_env_args(full_args);

    // catch_unwind surfaces magiskboot-rs panics as Err so the GUI stays alive.
    let args_repr = args.join(" ");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        magiskboot::cli::boot_main(cmds).unwrap_or(1)
    }));

    // Restore env regardless of outcome so a panicking magiskboot doesn't
    // leave KEEPVERITY set for unrelated callers.
    for (k, old) in &saved_envs {
        match old {
            Some(v) => unsafe { std::env::set_var(k, v) },
            None => unsafe { std::env::remove_var(k) },
        }
    }

    if let Some(dir) = original_dir {
        let _ = std::env::set_current_dir(dir);
    }

    match result {
        Ok(code) => Ok(code),
        Err(_) => Err(LtboxError::BootImage(format!(
            "magiskboot panicked while running: {args_repr}"
        ))),
    }
}

fn sha1_hash(data: &[u8]) -> String {
    use digest::Digest;
    let mut h = sha1::Sha1::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_ramdisk_name_uses_top_level_cpio() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("ramdisk.cpio"), b"top-level").unwrap();
        assert_eq!(root_ramdisk_name(temp.path()).unwrap(), "ramdisk.cpio");
    }

    #[test]
    fn root_ramdisk_name_rejects_missing_ramdisk() {
        let temp = tempfile::tempdir().unwrap();
        assert!(root_ramdisk_name(temp.path()).is_err());
    }
}

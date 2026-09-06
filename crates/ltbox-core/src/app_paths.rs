//! Cross-platform helper for LTBox-owned writable directories.
//!
//! Centralises every "where do generated outputs / dumps / backups
//! land?" question so call sites stop reaching for `current_exe()`.
//! Necessary because:
//!
//! * AppImage mounts the bundle read-only — writes next to the
//!   executable would either fail or land on the squashfs-backed
//!   FUSE mount, depending on kernel.
//! * Distro-installed binaries live under `/usr/bin`, owned by root,
//!   non-writable for normal users.
//! * Even on Windows, `Program Files`-installed copies hit UAC the
//!   moment something tries to write next to `ltbox.exe`.
//!
//! ## Per-OS layout
//!
//! Every platform uses its machine-local application data directory.
//!
//! | OS      | Auto-output / backup root             |
//! |---------|---------------------------------------|
//! | Windows | `%LOCALAPPDATA%\ltbox`                |
//! | Linux   | `$XDG_DATA_HOME/ltbox` (≈ `~/.local/share/ltbox`) |
//! | macOS   | `~/Library/Application Support/ltbox` |
//!
//! User-selected output folders (Partition Dump destination, Physical
//! Storage dump path, etc.) are NOT routed through here — those are
//! explicit picks and stay where the user chose.

#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;

/// Resolve an LTBox data root from the platform data directory. Keeping this
/// small transformation separate makes every OS path shape testable without
/// changing process environment variables.
fn data_root_from(data_dir: Option<PathBuf>) -> PathBuf {
    data_dir.unwrap_or_else(|| PathBuf::from(".")).join("ltbox")
}

/// Directory where auto-generated dumps / backups / per-action output
/// roots live. Caller `create_dir_all`s before writing.
///
/// Platform mapping documented at the module level.
pub fn auto_output_root() -> PathBuf {
    if cfg!(windows) {
        // `data_local_dir` resolves to `%LOCALAPPDATA%` on Windows. The
        // `./ltbox` fallback matches the existing non-Windows fallback shape.
        data_root_from(dirs::data_local_dir())
    } else {
        // dirs::data_dir() returns `$XDG_DATA_HOME` on Linux (default
        // `~/.local/share`) and `~/Library/Application Support` on
        // macOS. Matches what `settings_store` + the root pipeline's
        // `work_dir` already use elsewhere in the workspace.
        data_root_from(dirs::data_dir())
    }
}

/// Per-action output sub-directory under [`auto_output_root`].
///
/// `slug` is the action identifier (e.g. `"patch_arb"`,
/// `"region_convert"`).
pub fn auto_output_dir_for(slug: &str) -> PathBuf {
    auto_output_root().join("outputs").join(slug)
}

/// Root directory for per-operation device snapshots.
pub fn backup_root() -> PathBuf {
    auto_output_root().join("backup")
}

/// Per-flow exec-time scratch directory. Caller is responsible for
/// `remove_dir_all` on entry + `create_dir_all` before writes; this
/// helper only resolves the path. Slug is the flow identifier
/// (`"flash_arb"`, `"flash_country"`, `"root"`, …). Routes through
/// [`auto_output_root`] so packaged installs stay consistent with every other
/// LTBox-owned write.
pub fn work_dir_for(slug: &str) -> PathBuf {
    auto_output_root().join("work").join(slug)
}

/// Remove every exec-time scratch directory created by [`work_dir_for`].
/// Call on a *successful* operation so the `work/` scratch (firmware flash,
/// country change, ARB overlays, …) does not accumulate; a mid-flow abort
/// deliberately leaves it behind for inspection. Best-effort — errors ignored.
///
/// Direct removal only — no size accounting, so a successful op never pays for
/// a tree walk over (potentially large) decrypted images. `remove_dir_all`
/// refuses to follow a symlinked root, so this can't delete outside the LTBox
/// scratch tree. The Settings UI uses the separate
/// [`clean_temp_files_reporting`] when it needs a tally.
pub fn clean_work_dirs() {
    let _ = std::fs::remove_dir_all(auto_output_root().join("work"));
}

/// `true` only if `path` is a real directory — a symlink (even one pointing at
/// a directory) reports `false`. Uses `symlink_metadata` so the temp scan and
/// sweep never follow a symlinked root out of the LTBox tree.
fn is_real_dir(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false)
}

/// On-disk size in bytes of everything [`clean_temp_files_reporting`] would
/// remove. Drives the Settings button's enabled state and the size readout —
/// `0` means there is nothing to clean. `adb/` and `backup*` are never counted.
/// Symlinked roots are skipped, matching what the sweep can actually remove.
pub fn temp_files_size() -> u64 {
    let root = auto_output_root();
    let work = root.join("work");
    let outputs = root.join("outputs");
    let work_size = if is_real_dir(&work) {
        dir_size(&work)
    } else {
        0
    };
    let outputs_size = if is_real_dir(&outputs) {
        dir_size(&outputs)
    } else {
        0
    };
    work_size + outputs_size
}

/// Remove every temporary file the Settings "clean temporary files" action
/// targets — `work/` scratch + `outputs/` auto-output dirs — and report
/// `(removed_roots, freed_bytes)`. The persistent `adb/` key dir and all
/// `backup/` and legacy `backups/` dumps are left in place. Symlinked roots are skipped (never
/// followed), so the sweep can't escape the LTBox tree. Best-effort: a failed
/// delete is not counted.
pub fn clean_temp_files_reporting() -> (usize, u64) {
    let mut removed = 0usize;
    let mut freed = 0u64;
    let root = auto_output_root();
    for leaf in ["work", "outputs"] {
        let path = root.join(leaf);
        // Only act on a real directory — never follow a symlinked root
        // into unrelated user dirs.
        if !is_real_dir(&path) {
            continue;
        }
        let size = dir_size(&path);
        if std::fs::remove_dir_all(&path).is_ok() {
            removed += 1;
            freed += size;
        }
    }
    (removed, freed)
}

/// Recursive on-disk size of `path` in bytes. Best-effort: entries that can't
/// be read are skipped (counted as 0) rather than aborting the walk.
///
/// Symlinks are **not** followed: [`std::fs::DirEntry::file_type`] reports the
/// link itself, so a symlinked directory is treated as a link and skipped —
/// matching what `remove_dir_all` actually deletes, and avoiding both symlink
/// cycles and escaping the temp tree into unrelated directories.
fn dir_size(path: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            total += dir_size(&entry.path());
        } else if file_type.is_file() {
            // `metadata()` follows links, but we only reach it for a real
            // file here, so the size is the file's own.
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
        // Symlinks / special files contribute 0 — they aren't recursed into.
    }
    total
}

/// Path to LTBox's owned ADB RSA private key. Persisted so the user
/// only has to tap "Allow USB debugging?" once per device — `adb_client`'s
/// `usb` backend mints a fresh in-memory key whenever the key file is
/// missing, which would re-trigger the on-device prompt on every
/// `AdbManager::new()` if we let it fall through to the default
/// `~/.android/adbkey`.
///
/// Stored under [`auto_output_root`] / `adb/adbkey` so it inherits the
/// same OS-specific writable-directory split as every other LTBox
/// asset.
pub fn adb_key_path() -> PathBuf {
    auto_output_root().join("adb").join("adbkey")
}

#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MigrationDecision {
    Move,
    LeaveExisting,
    Nothing,
}

/// Pure migration gate shared by the filesystem implementation and tests.
#[cfg(any(windows, test))]
fn migration_decision(legacy_exists: bool, destination_exists: bool) -> MigrationDecision {
    if destination_exists {
        MigrationDecision::LeaveExisting
    } else if legacy_exists {
        MigrationDecision::Move
    } else {
        MigrationDecision::Nothing
    }
}

/// Map one legacy executable-adjacent path into the new data tree. This is
/// deliberately path-only so layout rules can be tested without user data.
#[cfg(any(windows, test))]
fn legacy_destination(legacy: &Path, data_root: &Path) -> Option<PathBuf> {
    let name = legacy.file_name()?.to_str()?;
    if name == "adbkey" && legacy.parent()?.file_name()?.to_str()? == "adb" {
        return Some(data_root.join("adb").join("adbkey"));
    }
    if let Some(slug) = name.strip_prefix("work_").filter(|s| !s.is_empty()) {
        return Some(data_root.join("work").join(slug));
    }
    if let Some(slug) = name.strip_prefix("output_").filter(|s| !s.is_empty()) {
        return Some(data_root.join("outputs").join(slug));
    }
    if name.starts_with("backup_") {
        return Some(data_root.join("backups").join(name));
    }
    None
}

#[cfg(any(windows, test))]
fn copy_entry(source: &Path, destination: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.file_type().is_file() {
        std::fs::copy(source, destination)?;
        return Ok(());
    }
    if !metadata.file_type().is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unsupported legacy entry type: {}", source.display()),
        ));
    }

    std::fs::create_dir(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn remove_entry_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(any(windows, test))]
fn copy_fallback_with<F>(source: &Path, destination: &Path, copy: F) -> std::io::Result<()>
where
    F: FnOnce(&Path, &Path) -> std::io::Result<()>,
{
    if let Err(copy_error) = copy(source, destination) {
        return match remove_entry_if_exists(destination) {
            Ok(()) => Err(copy_error),
            Err(cleanup_error) => Err(std::io::Error::other(format!(
                "{copy_error}; failed to clean partial destination {}: {cleanup_error}",
                destination.display()
            ))),
        };
    }

    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.file_type().is_dir() {
        std::fs::remove_dir_all(source)
    } else {
        std::fs::remove_file(source)
    }
}

#[cfg(any(windows, test))]
fn copy_fallback(source: &Path, destination: &Path) -> std::io::Result<()> {
    copy_fallback_with(source, destination, copy_entry)
}

#[cfg(any(windows, test))]
fn migrate_legacy_data(legacy_root: &Path, data_root: &Path) -> Vec<String> {
    if legacy_root == data_root {
        return Vec::new();
    }

    let mut candidates = vec![legacy_root.join("adb").join("adbkey")];
    if let Ok(entries) = std::fs::read_dir(legacy_root) {
        candidates.extend(entries.flatten().filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        }));
    }

    let mut failures = Vec::new();
    for legacy in candidates {
        let Some(destination) = legacy_destination(&legacy, data_root) else {
            continue;
        };
        if migration_decision(legacy.exists(), destination.exists()) != MigrationDecision::Move {
            continue;
        }
        let Some(parent) = destination.parent() else {
            continue;
        };
        if let Err(error) = std::fs::create_dir_all(parent) {
            failures.push(format!(
                "failed to prepare {} for legacy data migration from {}: {error}",
                parent.display(),
                legacy.display()
            ));
            continue;
        }
        if let Err(rename_error) = std::fs::rename(&legacy, &destination)
            && let Err(copy_error) = copy_fallback(&legacy, &destination)
        {
            failures.push(format!(
                "failed to migrate legacy data from {} to {}: rename failed: {rename_error}; copy fallback failed: {copy_error}",
                legacy.display(),
                destination.display()
            ));
        }
    }
    failures
}

/// Move legacy Windows data out of the executable directory. Each mapped item
/// is independently idempotent: an existing destination is always kept, while
/// a failed move is reported and does not prevent the app from starting.
#[cfg(windows)]
pub fn migrate_legacy_windows_data() -> Vec<String> {
    let Some(legacy_root) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
    else {
        return Vec::new();
    };
    migrate_legacy_data(&legacy_root, &auto_output_root())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: outputs should never land next to an installed binary.
    #[test]
    fn outputs_never_exe_adjacent() {
        let exe_parent = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from));
        let Some(exe_parent) = exe_parent else {
            return;
        };
        let dir = auto_output_dir_for("patch_arb");
        assert!(
            !dir.starts_with(&exe_parent),
            "auto_output_dir_for landed under exe parent: {} ⊂ {}",
            dir.display(),
            exe_parent.display(),
        );
    }

    /// Backup helper must mirror the same exe-adjacency rule.
    #[test]
    fn backups_never_exe_adjacent() {
        let exe_parent = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from));
        let Some(exe_parent) = exe_parent else {
            return;
        };
        let dir = backup_root();
        assert!(
            !dir.starts_with(&exe_parent),
            "backup_root landed under exe parent: {} ⊂ {}",
            dir.display(),
            exe_parent.display(),
        );
    }

    /// `dir_size` sums nested files; the cleanup tally relies on it being
    /// accurate before the tree is removed.
    #[test]
    fn dir_size_sums_nested_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::write(root.join("a.bin"), [0u8; 10]).expect("write a");
        let sub = root.join("nested");
        std::fs::create_dir_all(&sub).expect("mkdir nested");
        std::fs::write(sub.join("b.bin"), [0u8; 25]).expect("write b");
        assert_eq!(dir_size(root), 35);
        // Missing path measures as 0 rather than panicking.
        assert_eq!(dir_size(&root.join("does-not-exist")), 0);
    }

    /// Per-action dirs must be distinct so wizard outputs never
    /// collide.
    #[test]
    fn auto_output_dir_distinct_per_slug() {
        let a = auto_output_dir_for("patch_arb");
        let b = auto_output_dir_for("region_convert");
        assert_ne!(a, b);
    }

    /// Windows uses `%LOCALAPPDATA%\ltbox` and the same category layout as
    /// Linux/macOS.
    #[cfg(windows)]
    #[test]
    fn windows_uses_local_app_data_layout() {
        let expected = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ltbox");
        assert_eq!(auto_output_root(), expected);
        assert_eq!(work_dir_for("root"), expected.join("work").join("root"));
        assert_eq!(
            auto_output_dir_for("patch_arb"),
            expected.join("outputs").join("patch_arb")
        );
        assert_eq!(backup_root(), expected.join("backup"));
        assert_eq!(adb_key_path(), expected.join("adb").join("adbkey"));
    }

    #[test]
    fn platform_data_roots_keep_expected_shapes() {
        // Build the expectation with `join` too: the separator is the host's,
        // not the one belonging to the platform whose layout is being modelled,
        // so spelling it out fails everywhere except Windows.
        for platform_dir in [
            PathBuf::from(r"C:\Users\user\AppData\Local"),
            PathBuf::from("/home/user/.local/share"),
            PathBuf::from("/Users/user/Library/Application Support"),
        ] {
            let expected = platform_dir.join("ltbox");
            assert_eq!(data_root_from(Some(platform_dir)), expected);
        }
        assert_eq!(data_root_from(None), PathBuf::from(".").join("ltbox"));
    }

    #[test]
    fn legacy_paths_map_to_category_layout() {
        let legacy = Path::new("legacy");
        let root = Path::new("data").join("ltbox");
        assert_eq!(
            legacy_destination(&legacy.join("work_root"), &root),
            Some(root.join("work").join("root"))
        );
        assert_eq!(
            legacy_destination(&legacy.join("output_patch_arb"), &root),
            Some(root.join("outputs").join("patch_arb"))
        );
        assert_eq!(
            legacy_destination(&legacy.join("backup_init_boot"), &root),
            Some(root.join("backups").join("backup_init_boot"))
        );
        assert_eq!(
            legacy_destination(&legacy.join("adb").join("adbkey"), &root),
            Some(root.join("adb").join("adbkey"))
        );
    }

    #[test]
    fn migration_decision_moves_only_legacy_data_without_destination() {
        assert_eq!(migration_decision(true, false), MigrationDecision::Move);
        assert_eq!(
            migration_decision(true, true),
            MigrationDecision::LeaveExisting
        );
        assert_eq!(migration_decision(false, false), MigrationDecision::Nothing);
    }

    #[test]
    fn copy_fallback_moves_directory_tree_and_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_dir = temp.path().join("legacy-work");
        let destination_dir = temp.path().join("data").join("work");
        std::fs::create_dir_all(source_dir.join("nested")).expect("source tree");
        std::fs::create_dir_all(destination_dir.parent().expect("destination parent"))
            .expect("directory destination parent");
        std::fs::write(source_dir.join("root.bin"), b"root").expect("root file");
        std::fs::write(source_dir.join("nested").join("child.bin"), b"child").expect("nested file");

        copy_fallback(&source_dir, &destination_dir).expect("directory fallback");

        assert!(!source_dir.exists());
        assert_eq!(
            std::fs::read(destination_dir.join("root.bin")).expect("copied root file"),
            b"root"
        );
        assert_eq!(
            std::fs::read(destination_dir.join("nested").join("child.bin"))
                .expect("copied nested file"),
            b"child"
        );

        let source_file = temp.path().join("legacy-adbkey");
        let destination_file = temp.path().join("data").join("adb").join("adbkey");
        std::fs::create_dir_all(destination_file.parent().expect("file destination parent"))
            .expect("file destination parent");
        std::fs::write(&source_file, b"persistent-key").expect("source key");

        copy_fallback(&source_file, &destination_file).expect("file fallback");

        assert!(!source_file.exists());
        assert_eq!(
            std::fs::read(destination_file).expect("copied key"),
            b"persistent-key"
        );
    }

    #[test]
    fn failed_copy_fallback_removes_partial_destination_and_keeps_source() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("legacy-work");
        let destination = temp.path().join("data").join("work");
        std::fs::create_dir_all(&source).expect("source dir");
        std::fs::create_dir_all(destination.parent().expect("destination parent"))
            .expect("destination parent");
        std::fs::write(source.join("work.bin"), b"source").expect("source file");

        let result = copy_fallback_with(&source, &destination, |_, partial| {
            std::fs::create_dir(partial)?;
            std::fs::write(partial.join("partial.bin"), b"partial")?;
            Err(std::io::Error::other("injected copy failure"))
        });

        assert!(result.is_err());
        assert!(source.join("work.bin").exists());
        assert!(!destination.exists());
    }

    #[test]
    fn migration_moves_once_and_never_clobbers_existing_data() {
        let temp = tempfile::tempdir().expect("tempdir");
        let legacy = temp.path().join("legacy");
        let data = temp.path().join("local-app-data").join("ltbox");
        std::fs::create_dir_all(legacy.join("adb")).expect("legacy adb dir");
        std::fs::create_dir_all(legacy.join("work_root")).expect("legacy work dir");
        std::fs::create_dir_all(legacy.join("output_patch_arb")).expect("legacy output dir");
        std::fs::create_dir_all(legacy.join("backup_init_boot")).expect("legacy backup dir");
        std::fs::write(legacy.join("adb").join("adbkey"), b"legacy-key").expect("legacy key");
        std::fs::write(legacy.join("work_root").join("work.bin"), b"work").expect("legacy work");
        std::fs::write(
            legacy.join("output_patch_arb").join("output.bin"),
            b"legacy-output",
        )
        .expect("legacy output");
        std::fs::write(legacy.join("backup_init_boot").join("boot.img"), b"backup")
            .expect("legacy backup");

        // A pre-existing destination wins; its legacy peer remains untouched.
        std::fs::create_dir_all(data.join("outputs").join("patch_arb")).expect("new output dir");
        std::fs::write(
            data.join("outputs").join("patch_arb").join("output.bin"),
            b"new-output",
        )
        .expect("new output");

        assert!(migrate_legacy_data(&legacy, &data).is_empty());
        assert_eq!(
            std::fs::read(data.join("adb").join("adbkey")).expect("migrated key"),
            b"legacy-key"
        );
        assert!(data.join("work").join("root").join("work.bin").exists());
        assert!(
            data.join("backups")
                .join("backup_init_boot")
                .join("boot.img")
                .exists()
        );
        assert_eq!(
            std::fs::read(data.join("outputs").join("patch_arb").join("output.bin"))
                .expect("preserved output"),
            b"new-output"
        );
        assert!(legacy.join("output_patch_arb").exists());

        // A second pass has nothing left to move and preserves every result.
        assert!(migrate_legacy_data(&legacy, &data).is_empty());
        assert_eq!(
            std::fs::read(data.join("adb").join("adbkey")).expect("stable key"),
            b"legacy-key"
        );
    }
}

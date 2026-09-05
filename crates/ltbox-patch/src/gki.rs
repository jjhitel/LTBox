//! GKI custom-kernel root — replaces the kernel blob inside `boot.img`
//! with one supplied by the user. Two input shapes accepted:
//!
//! * **AnyKernel3 zip** (legacy / default) — `Image` / `kernel` / etc.
//!   pulled out via `extract_kernel_from_zip`.
//! * **Raw `boot.img`** — unpacked with magiskboot in a scratch subdir
//!   under `work_dir`, then the `kernel` it produces is copied over
//!   the stock kernel (`extract_kernel_from_boot_img`). Lets the user
//!   feed any donor boot image (custom kernel build, OEM custom rom
//!   boot.img, etc.) without first stripping it into an AnyKernel3
//!   wrapper.
//!
//! Selection is by file extension at the patcher entry — `.zip` →
//! AnyKernel3 path, `.img` → boot.img path. Other extensions error
//! out before the stock unpack runs.
//!
//! Writes `work_dir/new-boot.img` for the subsequent AVB re-sign step.

use fs_err as fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use ltbox_core::i18n::tr;
use ltbox_core::{LtboxError, Result};

use crate::boot;

/// Candidate kernel entry names inside AnyKernel3 zips; shortest path wins.
const CANDIDATES: &[&str] = &[
    "Image",
    "kernel",
    "Image.gz-dtb",
    "Image-dtb",
    "Image.gz",
    "zImage",
    "zImage-dtb",
];

/// Subdirectory under `work_dir` for unpacking a user-supplied
/// `boot.img` donor — isolated from the stock unpack so the two
/// `kernel` files don't collide and the stock dtb / ramdisk components
/// stay intact for the eventual repack.
const CUSTOM_BOOT_SUBDIR: &str = "custom_kernel_extract";

/// Patch `boot.img` inside `work_dir` with a kernel sourced from
/// `kernel_src`. Branches on the source extension:
///
/// * `.zip` → AnyKernel3 layout, kernel pulled from the zip entry.
/// * `.img` → boot.img donor, unpacked in a scratch subdir to extract
///   its `kernel` blob.
///
/// Any other extension errors out up-front (before the stock unpack
/// runs) so the user doesn't wait through a magiskboot pass only to
/// get a "wrong file type" message at the donor step.
///
/// On success, writes `work_dir/new-boot.img`.
pub fn patch_boot(work_dir: &Path, kernel_src: &Path, log: &mut Vec<String>) -> Result<PathBuf> {
    // Validate the donor extension before paying for the stock
    // `boot.img` unpack. GUI picker filters to `.zip` / `.img`, but a
    // typed path or a stale recent-paths entry could route anything
    // here; surface the input error immediately.
    let ext = kernel_src
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("zip") | Some("img") => {}
        other => {
            return Err(LtboxError::Patch(format!(
                "unsupported kernel source extension {:?} for {}; expected .zip or .img",
                other,
                kernel_src.display()
            )));
        }
    }

    let img_name = "boot.img";
    let img_path = work_dir.join(img_name);
    if !img_path.exists() {
        return Err(LtboxError::Patch(format!(
            "boot.img not found in {}",
            work_dir.display()
        )));
    }

    ltbox_core::live!(log, "[GKI] {}", tr("log_gki_unpack_boot"));
    boot::unpack(&img_path, work_dir)?;
    let kernel_dst = work_dir.join("kernel");
    if !kernel_dst.exists() {
        return Err(LtboxError::Patch(
            "magiskboot unpack did not produce a `kernel` file — boot image has no kernel".into(),
        ));
    }

    // Kernel-version sanity check — diagnostic only, catches wrong-kernel-family zips.
    if let Some(ver) = extract_linux_version(&kernel_dst) {
        ltbox_core::live!(log, "[GKI] {}: {ver}", tr("log_gki_stock_kver"));
    } else {
        ltbox_core::live!(log, "[GKI] {}", tr("log_gki_stock_kver_missing"));
    }

    ltbox_core::live!(
        log,
        "[GKI] {} {}",
        tr("log_gki_extracting_kernel"),
        kernel_src.display()
    );
    match ext.as_deref() {
        Some("zip") => extract_kernel_from_zip(kernel_src, &kernel_dst, log)?,
        // Whitelist-validated above; the remaining variant is `.img`.
        _ => extract_kernel_from_boot_img(kernel_src, &kernel_dst, work_dir, log)?,
    }

    if let Some(ver) = extract_linux_version(&kernel_dst) {
        ltbox_core::live!(log, "[GKI] {}: {ver}", tr("log_gki_replacement_kver"));
    } else {
        ltbox_core::live!(log, "[GKI] {}", tr("log_gki_replacement_kver_missing"));
    }

    ltbox_core::live!(log, "[GKI] {}", tr("log_gki_repack_boot"));
    boot::repack(img_name, work_dir)?;
    let new_boot = work_dir.join("new-boot.img");
    if !new_boot.exists() {
        return Err(LtboxError::Patch(
            "magiskboot repack produced no new-boot.img".into(),
        ));
    }
    ltbox_core::live!(log, "[GKI] {}", tr("log_patch_complete"));
    Ok(new_boot)
}

/// Scan kernel binary for `"Linux version X.Y.Z …"` banner; returns `X.Y.Z`
/// or `None` if absent. Caller treats `None` as inconclusive.
fn extract_linux_version(kernel_path: &Path) -> Option<String> {
    let data = fs::read(kernel_path).ok()?;
    const MARKER: &[u8] = b"Linux version ";
    let idx = data.windows(MARKER.len()).position(|w| w == MARKER)?;
    let rest = &data[idx + MARKER.len()..];
    // Stop at first non-digit/dot to avoid trailing `(builder@host) #...` cruft.
    let mut out = String::new();
    for b in rest.iter().take(64) {
        if b.is_ascii_digit() || *b == b'.' {
            out.push(*b as char);
        } else {
            break;
        }
    }
    // Require two dots so partial matches don't slip through.
    if out.chars().filter(|c| *c == '.').count() >= 2 {
        Some(out)
    } else {
        None
    }
}

/// Pull the kernel out of a user-supplied donor `boot.img` and copy
/// it over `dst`. Unpacks the donor in an isolated subdir under
/// `work_dir` so the parallel stock unpack (which already produced
/// `work_dir/kernel`, `ramdisk.cpio`, dtb, …) isn't clobbered — only
/// the `kernel` artefact is transplanted, the rest of the boot image
/// keeps its stock layout.
///
/// The subdir is wiped on entry so a previous run's leftovers can't
/// taint the unpack, and cleaned up on success. A failure leaves the
/// subdir in place for post-mortem.
fn extract_kernel_from_boot_img(
    boot_img: &Path,
    dst: &Path,
    work_dir: &Path,
    log: &mut Vec<String>,
) -> Result<()> {
    let scratch = work_dir.join(CUSTOM_BOOT_SUBDIR);
    if scratch.exists() {
        fs::remove_dir_all(&scratch)
            .map_err(|e| LtboxError::Patch(format!("clear scratch {}: {e}", scratch.display())))?;
    }
    fs::create_dir_all(&scratch)
        .map_err(|e| LtboxError::Patch(format!("create scratch {}: {e}", scratch.display())))?;
    boot::unpack(boot_img, &scratch)?;
    let donor_kernel = scratch.join("kernel");
    if !donor_kernel.exists() {
        return Err(LtboxError::Patch(format!(
            "donor boot image {} produced no `kernel` after magiskboot unpack — \
             not a recognised boot image format",
            boot_img.display()
        )));
    }
    let copied = fs::copy(&donor_kernel, dst)
        .map_err(|e| LtboxError::Patch(format!("copy donor kernel: {e}")))?;
    // Best-effort cleanup; leaving the subdir behind on a remove
    // failure is harmless — the next run wipes it anyway.
    let _ = fs::remove_dir_all(&scratch);
    ltbox_core::live!(
        log,
        "[GKI] {} {} → kernel ({} bytes)",
        tr("log_gki_staged"),
        boot_img.display(),
        copied
    );
    Ok(())
}

fn extract_kernel_from_zip(zip_path: &Path, dst: &Path, log: &mut Vec<String>) -> Result<()> {
    let file =
        fs::File::open(zip_path).map_err(|e| LtboxError::Patch(format!("open kernel zip: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| LtboxError::Patch(format!("kernel zip read: {e}")))?;

    // Per candidate: try exact archive-root match first, then basename match
    // preferring the shortest path so root `Image` beats `subdir/Image`.
    let names: Vec<String> = archive
        .file_names()
        .filter(|n| !n.ends_with('/'))
        .map(|s| s.to_string())
        .collect();

    let mut picked: Option<String> = None;
    'outer: for candidate in CANDIDATES {
        if let Some(n) = names.iter().find(|n| n.as_str() == *candidate) {
            picked = Some(n.clone());
            break;
        }
        let mut matches: Vec<&String> =
            names.iter().filter(|n| basename(n) == *candidate).collect();
        if !matches.is_empty() {
            matches.sort_by_key(|n| (n.matches('/').count(), n.to_lowercase()));
            picked = Some(matches[0].clone());
            break 'outer;
        }
    }

    let name = picked.ok_or_else(|| {
        LtboxError::Patch(format!(
            "No kernel entry in {} (looked for {:?})",
            zip_path.display(),
            CANDIDATES
        ))
    })?;

    let mut entry = archive
        .by_name(&name)
        .map_err(|e| LtboxError::Patch(format!("kernel zip {name}: {e}")))?;
    // Stream the zip entry to disk instead of buffering it whole. The
    // previous `Vec::with_capacity(entry.size() as usize)` trusted the
    // local zip header's declared size, so a malformed or hostile
    // AnyKernel zip could declare an enormous kernel and force an OOM
    // before any bytes were read. A sane upper bound (200 MiB — well
    // above any real Android boot kernel) protects against a runaway
    // copy if the entry's actual stream is malformed too.
    const MAX_KERNEL_BYTES: u64 = 200 * 1024 * 1024;
    let mut out = fs::File::create(dst)?;
    let copied = {
        // `take(MAX + 1)` so an exactly-MAX-byte kernel reads through
        // without flagging, and only a > MAX byte stream surfaces as
        // the cap error (avoids a false positive on a real 200 MiB
        // image that happens to land on the boundary).
        let mut limited = (&mut entry).take(MAX_KERNEL_BYTES + 1);
        std::io::copy(&mut limited, &mut out)?
    };
    if copied > MAX_KERNEL_BYTES {
        return Err(LtboxError::Patch(format!(
            "kernel zip entry {name} exceeds {MAX_KERNEL_BYTES} byte cap; \
             refusing to stage"
        )));
    }
    drop(entry);
    ltbox_core::live!(
        log,
        "[GKI] {} {name} → kernel ({} bytes)",
        tr("log_gki_staged"),
        copied
    );
    Ok(())
}

fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(idx) => &path[idx + 1..],
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_standard_banner() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(
            tmp.path(),
            b"\x00\x00padding\x00Linux version 6.6.118-android15-8-gabc (builder@host) ...",
        )
        .unwrap();
        assert_eq!(
            extract_linux_version(tmp.path()).as_deref(),
            Some("6.6.118")
        );
    }

    #[test]
    fn rejects_two_segment_version() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), b"junk Linux version 6.6 trailing").unwrap();
        assert_eq!(extract_linux_version(tmp.path()), None);
    }

    #[test]
    fn returns_none_without_banner() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), b"ELF\x7f random bytes no banner").unwrap();
        assert_eq!(extract_linux_version(tmp.path()), None);
    }
}

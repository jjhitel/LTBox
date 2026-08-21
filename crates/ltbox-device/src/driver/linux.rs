//! Linux device-access provisioning: detect and install the LTBox udev rules
//! that grant the desktop user libusb / serial access to the Qualcomm EDL
//! (`05c6:9008`), Lenovo (`17ef`), and Google ADB (`18d1`) USB IDs.
//! See `misc/udev/51-ltbox-qcom.rules`.
//!
//! Userspace mode has no driver *download*: the rules ship embedded in the
//! binary and are written by the privileged `ltbox --install-udev` entry
//! point. Kernel mode uses Qualcomm's `qcom-usb-kernel-drivers` Debian package
//! (`qud`) when `dpkg` is available.
//!
//! Deferred until a Lenovo Qualcomm target is available on Linux: the
//! `/sys/bus/usb/devices` walk for `05c6:9008` + serial-node permission test
//! (a `DevicePresentNoPermission`-style state). Rules presence / staleness is
//! pure filesystem state and is implemented + tested here today.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ltbox_core::{live, tr_args};

use super::{
    DriverError, DriverStatus, DriverUpdate, Result, classify_udev_rules, parse_version,
    qcom_driver_mode, version_from_tag, version_lt,
};

/// Where `ltbox --install-udev` writes the rules; kept in sync with the GUI's
/// `UDEV_RULES_PATH`.
const UDEV_RULES_PATH: &str = "/etc/udev/rules.d/51-ltbox-qcom.rules";
const KERNEL_RELEASES_API: &str =
    "https://api.github.com/repos/qualcomm/qcom-usb-kernel-drivers/releases?per_page=10";
const LINUX_TAG_NEEDLE: &str = "lnx";
const KERNEL_DEB_PACKAGE: &str = "qud";
const MAX_KERNEL_DRIVER_ZIP_BYTES: u64 = 256 * 1024 * 1024;
const MAX_KERNEL_DEB_BYTES: u64 = 256 * 1024 * 1024;

/// System directories trusted for elevated helpers (`pkexec`, elevated `dpkg`).
/// Never search arbitrary `PATH` for tools that will run with root privileges.
const TRUSTED_BIN_DIRS: &[&str] = &["/usr/bin", "/bin", "/usr/sbin", "/sbin"];

#[derive(Debug, serde::Deserialize)]
struct GithubRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: String,
}

#[derive(Debug, serde::Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

pub fn check_required_drivers() -> DriverStatus {
    if qcom_driver_mode().is_kernel() {
        return check_kernel_driver();
    }
    check_udev_rules()
}

fn check_udev_rules() -> DriverStatus {
    match std::fs::read_to_string(UDEV_RULES_PATH) {
        Ok(content) => classify_udev_rules(Some(&content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => classify_udev_rules(None),
        // File exists but is unreadable (permission or otherwise) — surface a
        // repairable state rather than silently claiming the rules are fine.
        Err(_) => DriverStatus::UdevRulesNoPermission,
    }
}

fn check_kernel_driver() -> DriverStatus {
    if which_program("dpkg-query").is_none() {
        return DriverStatus::KernelDriverUnsupported;
    }
    if installed_kernel_driver_version().is_some() {
        DriverStatus::Present
    } else {
        DriverStatus::KernelDriverMissing
    }
}

pub fn check_driver_update() -> Option<DriverUpdate> {
    if !qcom_driver_mode().is_kernel() {
        return None;
    }
    let current = installed_kernel_driver_version()?;
    let (tag, _asset) = fetch_latest_linux_kernel_release().ok()?;
    let latest = version_from_tag(&tag)?;
    if version_lt(&current, &latest) {
        Some(DriverUpdate { current, latest })
    } else {
        None
    }
}

/// Install (or refresh) the udev rules by re-launching this binary through
/// `pkexec` with the fixed `--install-udev` flag. Only the binary's own
/// resolved path is passed — never user input.
pub fn download_and_install(log: &mut Vec<String>) -> Result<()> {
    if qcom_driver_mode().is_kernel() {
        install_kernel_driver(log)
    } else {
        install_udev_rules(log)
    }
}

fn install_udev_rules(log: &mut Vec<String>) -> Result<()> {
    if check_udev_rules() == DriverStatus::Present {
        log.push("[driver] udev rules already up to date".to_string());
        return Ok(());
    }

    let exe = std::env::current_exe().map_err(|e| {
        DriverError::Io(std::io::Error::new(
            e.kind(),
            format!("cannot resolve the LTBox executable path: {e}"),
        ))
    })?;
    if !exe.is_file() {
        return Err(DriverError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("LTBox executable not found at {}", exe.display()),
        )));
    }

    // Require pkexec — never silently fall back to a terminal `sudo` from the
    // GUI, which has no controlling terminal to prompt on.
    let pkexec = resolve_trusted_executable("pkexec").map_err(|e| {
        map_not_found(
            e,
            "pkexec not found — install polkit, or run `sudo ltbox --install-udev` in a terminal",
        )
    })?;

    log.push(format!("[driver] pkexec {} --install-udev", exe.display()));
    let status = std::process::Command::new(pkexec)
        .arg(&exe)
        .arg("--install-udev")
        .status()
        .map_err(|e| DriverError::Io(std::io::Error::new(e.kind(), format!("pkexec: {e}"))))?;

    match status.code() {
        Some(0) => {}
        // polkit authorization denied / dialog dismissed → pkexec exits 126/127.
        Some(126 | 127) => return Err(DriverError::InstallCancelled),
        Some(code) => return Err(DriverError::InstallerFailed { exit_code: code }),
        None => {
            return Err(DriverError::Io(std::io::Error::other(
                "pkexec terminated by a signal",
            )));
        }
    }

    // Confirm the write actually landed before reporting success.
    if check_required_drivers() != DriverStatus::Present {
        return Err(DriverError::Io(std::io::Error::other(
            "udev rules still not in place after install",
        )));
    }
    log.push("[driver] udev rules installed".to_string());
    Ok(())
}

fn install_kernel_driver(log: &mut Vec<String>) -> Result<()> {
    let pkexec = resolve_trusted_executable("pkexec").map_err(|e| {
        map_not_found(
            e,
            "pkexec not found — install polkit or install the Qualcomm kernel driver package manually",
        )
    })?;
    // Elevated target: never honor an attacker-controlled `dpkg` from PATH.
    let dpkg = resolve_trusted_executable("dpkg").map_err(|e| {
        map_not_found(
            e,
            "dpkg not found — automatic Linux kernel-driver install is only supported on Debian-style systems",
        )
    })?;

    live!(
        log,
        "[Driver] {}",
        ltbox_core::i18n::tr("live_driver_fetch_meta")
    );
    let (tag, asset) = fetch_latest_linux_kernel_release()?;
    live!(
        log,
        "[Driver] {}",
        tr_args!("live_driver_asset", name = &asset.name)
    );

    // Private, exclusive temp dir under the process temp root. A predictable
    // `temp_dir()/ltbox_*_{pid}` path is a classic local symlink / content-
    // swap race before the elevated `pkexec dpkg -i`; create_dir with a
    // unique name + owner-only mode rejects pre-created paths and keeps the
    // downloaded package private until install completes.
    let tmp_dir = PrivateTempDir::create("ltbox_qcom_kernel_drv")?;
    let zip_path = tmp_dir.path().join(&asset.name);
    let deb_path = tmp_dir
        .path()
        .join(format!("{KERNEL_DEB_PACKAGE}_{tag}.deb"));
    let result = (|| {
        if asset.size > MAX_KERNEL_DRIVER_ZIP_BYTES {
            return Err(DriverError::Parse(format!(
                "driver asset too large: {} bytes",
                asset.size
            )));
        }
        download_file(&asset.browser_download_url, &asset.name, &zip_path, log)?;
        extract_first_deb(&zip_path, &deb_path)?;
        live!(
            log,
            "[Driver] {}",
            ltbox_core::i18n::tr("live_driver_running_package_installer")
        );
        let status = std::process::Command::new(pkexec)
            .arg(dpkg)
            .arg("-i")
            .arg(&deb_path)
            .status()
            .map_err(|e| DriverError::Io(std::io::Error::new(e.kind(), format!("pkexec: {e}"))))?;
        match status.code() {
            Some(0) => {}
            Some(126 | 127) => return Err(DriverError::InstallCancelled),
            Some(code) => return Err(DriverError::InstallerFailed { exit_code: code }),
            None => {
                return Err(DriverError::Io(std::io::Error::other(
                    "pkexec terminated by a signal",
                )));
            }
        }
        if check_kernel_driver() != DriverStatus::Present {
            return Err(DriverError::Io(std::io::Error::other(
                "kernel driver package still not installed after installer finished",
            )));
        }
        live!(
            log,
            "[Driver] {}",
            ltbox_core::i18n::tr("live_driver_install_finished")
        );
        Ok(())
    })();
    drop(tmp_dir);
    result
}

/// Owner-only scratch directory that is removed on drop.
struct PrivateTempDir {
    path: PathBuf,
}

impl PrivateTempDir {
    fn create(prefix: &str) -> Result<Self> {
        let path = create_private_temp_dir(prefix)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PrivateTempDir {
    fn drop(&mut self) {
        cleanup(&self.path);
    }
}

fn unique_temp_token() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos}-{seq}", std::process::id())
}

/// Create an exclusive, non-symlink directory under `std::env::temp_dir()`
/// with owner-only permissions on Unix. Retries on name collision.
fn create_private_temp_dir(prefix: &str) -> Result<PathBuf> {
    let base = std::env::temp_dir();
    for _ in 0..32 {
        let path = base.join(format!("{prefix}_{}", unique_temp_token()));
        match create_exclusive_private_dir(&path) {
            Ok(()) => {
                verify_private_dir(&path)?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(DriverError::Io(std::io::Error::new(
                    e.kind(),
                    format!("create private temp dir {}: {e}", path.display()),
                )));
            }
        }
    }
    Err(DriverError::Io(std::io::Error::other(format!(
        "exhausted unique names while creating private temp dir under {}",
        base.display()
    ))))
}

fn create_exclusive_private_dir(path: &Path) -> std::io::Result<()> {
    // `create_dir` (not `create_dir_all`) is exclusive: fails if the path
    // already exists, including as a symlink planted by another user.
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(path)
    }
}

fn verify_private_dir(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| {
        DriverError::Io(std::io::Error::new(
            e.kind(),
            format!("stat private temp dir {}: {e}", path.display()),
        ))
    })?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(DriverError::Io(std::io::Error::other(format!(
            "private temp path {} is not a plain directory",
            path.display()
        ))));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Owner-only: reject group/other bits so a pre-existing open umask or
        // unexpected mode does not leave the package world-readable before
        // `pkexec`. We created this directory moments ago, so ownership is
        // the creating process's euid by construction.
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(DriverError::Io(std::io::Error::other(format!(
                "private temp dir {} has overly permissive mode {mode:o}",
                path.display()
            ))));
        }
    }
    Ok(())
}

fn fetch_latest_linux_kernel_release() -> Result<(String, GithubAsset)> {
    let meta_agent = ltbox_core::downloader::build_agent();
    let releases: Vec<GithubRelease> = meta_agent
        .get(KERNEL_RELEASES_API)
        .call()?
        .body_mut()
        .read_json()
        .map_err(|e| DriverError::Parse(e.to_string()))?;

    let release = select_latest_linux_kernel_release(releases).ok_or(DriverError::NoAsset)?;
    let tag = release.tag_name.clone();
    let asset = release
        .assets
        .into_iter()
        .find(|a| linux_kernel_asset_matches(&a.name))
        .ok_or(DriverError::NoAsset)?;
    Ok((tag, asset))
}

/// Pick the newest published stable Linux kernel release that ships a QUD zip.
/// Drafts and prereleases are rejected before tag/asset matching; among the
/// remaining candidates the highest `published_at` wins.
fn select_latest_linux_kernel_release(releases: Vec<GithubRelease>) -> Option<GithubRelease> {
    let mut matching: Vec<GithubRelease> = releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter(|r| r.tag_name.to_ascii_lowercase().contains(LINUX_TAG_NEEDLE))
        .filter(|r| r.assets.iter().any(|a| linux_kernel_asset_matches(&a.name)))
        .collect();
    matching.sort_unstable_by(|a, b| b.published_at.cmp(&a.published_at));
    matching.into_iter().next()
}

fn linux_kernel_asset_matches(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("qud_") && lower.ends_with("_all.zip")
}

fn installed_kernel_driver_version() -> Option<String> {
    let dpkg_query = which_program("dpkg-query")?;
    let out = std::process::Command::new(dpkg_query)
        .arg("-W")
        .arg("-f=${Version}")
        .arg(KERNEL_DEB_PACKAGE)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if parse_version(&version).is_some() {
        Some(version)
    } else {
        None
    }
}

fn download_file(
    url: &str,
    name: &str,
    dst: &std::path::Path,
    log: &mut Vec<String>,
) -> Result<()> {
    use ltbox_core::downloader::{DownloadEvent, stream_with_progress};

    let agent = ltbox_core::downloader::build_agent();
    let display_name = name.to_string();
    stream_with_progress(&agent, url, dst, log, move |log, event| match event {
        DownloadEvent::Start => {
            live!(
                log,
                "[Driver] {}",
                tr_args!("live_driver_downloading", name = &display_name)
            );
        }
        DownloadEvent::ProgressPct {
            downloaded_mb,
            total_mb,
            pct,
            speed_mbps,
        } => {
            live!(
                log,
                "[Driver] {}",
                tr_args!(
                    "live_driver_progress_pct",
                    name = &display_name,
                    pct = format!("{pct:>3}"),
                    downloaded = format!("{downloaded_mb:.1}"),
                    total = format!("{total_mb:.1}"),
                    speed = format!("{speed_mbps:.1}"),
                )
            );
        }
        DownloadEvent::ProgressChunked {
            downloaded_mb,
            speed_mbps,
        } => {
            live!(
                log,
                "[Driver] {}",
                tr_args!(
                    "live_driver_progress_chunked",
                    name = &display_name,
                    downloaded = format!("{downloaded_mb:.1}"),
                    speed = format!("{speed_mbps:.1}"),
                )
            );
        }
        DownloadEvent::Done {
            downloaded_mb,
            elapsed_s,
        } => {
            live!(
                log,
                "[Driver] {}",
                tr_args!(
                    "live_driver_dl_done",
                    name = &display_name,
                    size = format!("{downloaded_mb:.1}"),
                    elapsed = format!("{elapsed_s:.1}"),
                )
            );
        }
    })
    .map_err(|e| DriverError::Http(format!("download: {e}")))
}

fn extract_first_deb(zip_path: &std::path::Path, out_path: &std::path::Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(DriverError::Zip)?;
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).map_err(DriverError::Zip)?;
        if !member.is_file() {
            continue;
        }
        let Some(path) = member.enclosed_name() else {
            continue;
        };
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("deb"))
        {
            if member.size() > MAX_KERNEL_DEB_BYTES {
                return Err(DriverError::Parse(format!(
                    "driver package too large: {} bytes",
                    member.size()
                )));
            }
            let mut out = create_private_file(out_path)?;
            let copied = std::io::copy(
                &mut std::io::Read::by_ref(&mut member).take(MAX_KERNEL_DEB_BYTES + 1),
                &mut out,
            )
            .map_err(|e| DriverError::Http(format!("extract: {e}")))?;
            if copied > MAX_KERNEL_DEB_BYTES {
                return Err(DriverError::Parse(format!(
                    "driver package too large: {copied} bytes"
                )));
            }
            out.flush()?;
            return Ok(());
        }
    }
    Err(DriverError::NoAsset)
}

fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::File::create(path)
    }
}

fn cleanup(path: &std::path::Path) {
    if let Err(e) = std::fs::remove_dir_all(path) {
        tracing::debug!("failed to clean driver temp dir {}: {e}", path.display());
    }
}

/// Whether `dpkg-query` is on `PATH` — the signal that this Linux host is
/// Debian-style and can use the Qualcomm kernel driver. Mirrors the gate in
/// [`check_kernel_driver`] and backs [`super::kernel_mode_supported`].
pub(super) fn dpkg_available() -> bool {
    which_program("dpkg-query").is_some()
}

fn map_not_found(err: DriverError, hint: &str) -> DriverError {
    match err {
        DriverError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => DriverError::Io(
            std::io::Error::new(std::io::ErrorKind::NotFound, hint.to_string()),
        ),
        other => other,
    }
}

/// Resolve `name` only from [`TRUSTED_BIN_DIRS`], then canonicalize and
/// validate it as a safe elevated helper. Used for `pkexec` and for the
/// elevated `dpkg` target passed to `pkexec` — never for plain availability
/// probes like `dpkg-query`.
fn resolve_trusted_executable(name: &str) -> Result<PathBuf> {
    for dir in TRUSTED_BIN_DIRS {
        let candidate = Path::new(dir).join(name);
        // Missing is fine — try the next trusted directory. An existing but
        // untrusted candidate must not fall through to a later path.
        match std::fs::symlink_metadata(&candidate) {
            Ok(_) => return validate_trusted_executable(&candidate),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(DriverError::Io(std::io::Error::new(
                    e.kind(),
                    format!("stat {}: {e}", candidate.display()),
                )));
            }
        }
    }
    Err(DriverError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!(
            "{name} not found under trusted system paths ({})",
            TRUSTED_BIN_DIRS.join(", ")
        ),
    )))
}

/// Canonicalize `path` and require a regular, root-owned executable that is
/// not group/other-writable and still lives under a trusted system bin
/// directory after symlink resolution (covers usrmerge `/bin` → `/usr/bin`).
fn validate_trusted_executable(path: &Path) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(path).map_err(|e| {
        DriverError::Io(std::io::Error::new(
            e.kind(),
            format!("cannot resolve {}: {e}", path.display()),
        ))
    })?;

    if !is_trusted_bin_path(&canonical) {
        return Err(DriverError::Io(std::io::Error::other(format!(
            "{} resolves outside trusted system directories to {}",
            path.display(),
            canonical.display()
        ))));
    }

    let meta = std::fs::metadata(&canonical).map_err(|e| {
        DriverError::Io(std::io::Error::new(
            e.kind(),
            format!("stat {}: {e}", canonical.display()),
        ))
    })?;
    if !meta.is_file() {
        return Err(DriverError::Io(std::io::Error::other(format!(
            "{} is not a regular file",
            canonical.display()
        ))));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = meta.permissions().mode();
        if !is_safe_privileged_exec_meta(meta.uid(), mode) {
            return Err(DriverError::Io(std::io::Error::other(format!(
                "{} is not a root-owned, non-group/other-writable executable (uid {}, mode {:o})",
                canonical.display(),
                meta.uid(),
                mode & 0o777
            ))));
        }
    }

    Ok(canonical)
}

/// True when `canonical` is a direct child of a trusted bin directory, or of
/// that directory after symlink resolution (usrmerge).
fn is_trusted_bin_path(canonical: &Path) -> bool {
    let Some(parent) = canonical.parent() else {
        return false;
    };
    TRUSTED_BIN_DIRS.iter().any(|dir| {
        let trusted = Path::new(dir);
        parent == trusted
            || std::fs::canonicalize(trusted)
                .ok()
                .is_some_and(|canon_dir| parent == canon_dir.as_path())
    })
}

/// Root-owned, not group/other-writable, and executable. Pure so unit tests
/// do not need a root-owned file on the development host.
fn is_safe_privileged_exec_meta(uid: u32, mode: u32) -> bool {
    // Require at least one execute bit so the helper matches its name and the
    // elevated-helper error path ("… executable") rather than accepting a
    // root-owned non-writable data file such as mode 0644.
    uid == 0 && (mode & 0o022) == 0 && (mode & 0o111) != 0
}

/// Locate `name` on `PATH` without pulling in a `which` dependency.
/// Used only for non-elevated probes (`dpkg-query` availability / version).
fn which_program(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_latest_linux_kernel_release_ignores_drafts_and_prereleases() {
        let releases = vec![
            GithubRelease {
                tag_name: "release-lnx-v9.9.9.9".into(),
                assets: vec![GithubAsset {
                    name: "qud_9.9.9.9_all.zip".into(),
                    browser_download_url: "https://example.test/prerelease".into(),
                    size: 1,
                }],
                draft: false,
                prerelease: true,
                published_at: "2026-07-20T00:00:00Z".into(),
            },
            GithubRelease {
                tag_name: "release-lnx-v8.8.8.8".into(),
                assets: vec![GithubAsset {
                    name: "qud_8.8.8.8_all.zip".into(),
                    browser_download_url: "https://example.test/draft".into(),
                    size: 1,
                }],
                draft: true,
                prerelease: false,
                published_at: "2026-07-19T00:00:00Z".into(),
            },
            GithubRelease {
                tag_name: "release-lnx-v1.0.5.0".into(),
                assets: vec![GithubAsset {
                    name: "qud_1.0.5.0_all.zip".into(),
                    browser_download_url: "https://example.test/older-stable".into(),
                    size: 1,
                }],
                draft: false,
                prerelease: false,
                published_at: "2026-07-10T00:00:00Z".into(),
            },
            GithubRelease {
                tag_name: "release-lnx-v1.0.6.4".into(),
                assets: vec![GithubAsset {
                    name: "qud_1.0.6.4_all.zip".into(),
                    browser_download_url: "https://example.test/latest-stable".into(),
                    size: 1,
                }],
                draft: false,
                prerelease: false,
                published_at: "2026-07-15T00:00:00Z".into(),
            },
            GithubRelease {
                tag_name: "release-win-v2.0.0.0".into(),
                assets: vec![GithubAsset {
                    name: "qud_2.0.0.0_all.zip".into(),
                    browser_download_url: "https://example.test/windows-only".into(),
                    size: 1,
                }],
                draft: false,
                prerelease: false,
                published_at: "2026-07-18T00:00:00Z".into(),
            },
        ];
        let selected = select_latest_linux_kernel_release(releases)
            .expect("stable Linux kernel release with matching asset");
        assert_eq!(selected.tag_name, "release-lnx-v1.0.6.4");
        assert_eq!(
            selected.assets[0].browser_download_url,
            "https://example.test/latest-stable"
        );
    }

    #[test]
    fn kernel_asset_matcher_accepts_qud_all_zip_only() {
        assert!(linux_kernel_asset_matches("qud_1.0.6.4_all.zip"));
        assert!(linux_kernel_asset_matches("QUD_1.0.6.4_ALL.ZIP"));
        assert!(!linux_kernel_asset_matches(
            "qud-win-v1.00.94.6_x86_64_arm64_signed.zip"
        ));
        assert!(!linux_kernel_asset_matches(
            "qcom_usb_kernel_drivers_x64.exe"
        ));
        assert!(!linux_kernel_asset_matches("qud_1.0.6.4_amd64.deb"));
    }

    #[test]
    fn private_temp_dir_is_exclusive_and_cleaned_on_drop() {
        let dir = PrivateTempDir::create("ltbox_qcom_kernel_drv_test").expect("create temp dir");
        let path = dir.path().to_path_buf();
        assert!(path.is_dir());
        assert!(
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("ltbox_qcom_kernel_drv_test_"))
        );

        // Exclusive create must refuse a pre-existing path (race resistance).
        let conflict = create_exclusive_private_dir(&path);
        assert!(conflict.is_err());
        assert_eq!(
            conflict.unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "owner-only temp dir");
        }

        drop(dir);
        assert!(
            !path.exists(),
            "PrivateTempDir drop must remove the scratch tree"
        );
    }

    #[test]
    fn private_temp_dir_rejects_non_directory_path() {
        let base = std::env::temp_dir().join(format!(
            "ltbox_qcom_kernel_drv_file_{}",
            unique_temp_token()
        ));
        std::fs::write(&base, b"not-a-dir").expect("write decoy file");
        let err = verify_private_dir(&base).expect_err("file must not pass dir verification");
        let _ = std::fs::remove_file(&base);
        assert!(err.to_string().contains("not a plain directory"));
    }

    #[test]
    fn privileged_exec_meta_requires_root_not_writable_and_executable() {
        assert!(is_safe_privileged_exec_meta(0, 0o755));
        assert!(is_safe_privileged_exec_meta(0, 0o555));
        assert!(is_safe_privileged_exec_meta(0, 0o711));
        assert!(is_safe_privileged_exec_meta(0, 0o700));
        assert!(is_safe_privileged_exec_meta(0, 0o100));
        assert!(!is_safe_privileged_exec_meta(1000, 0o755));
        assert!(!is_safe_privileged_exec_meta(0, 0o775));
        assert!(!is_safe_privileged_exec_meta(0, 0o757));
        assert!(!is_safe_privileged_exec_meta(0, 0o722));
        assert!(!is_safe_privileged_exec_meta(0, 0o644));
        assert!(!is_safe_privileged_exec_meta(0, 0o600));
        assert!(!is_safe_privileged_exec_meta(0, 0o444));
        assert!(!is_safe_privileged_exec_meta(0, 0o000));
    }

    #[test]
    fn validate_trusted_executable_rejects_user_owned_temp_file() {
        let path =
            std::env::temp_dir().join(format!("ltbox_trusted_exec_probe_{}", unique_temp_token()));
        std::fs::write(&path, b"#!/bin/sh\n").expect("write probe file");
        let err = validate_trusted_executable(&path)
            .expect_err("user temp file must not pass trusted validation");
        let _ = std::fs::remove_file(&path);
        let msg = err.to_string();
        assert!(
            msg.contains("outside trusted system directories")
                || msg.contains("not a root-owned")
                || msg.contains("not a regular file"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn map_not_found_preserves_non_not_found_errors() {
        let denied = DriverError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied",
        ));
        let mapped = map_not_found(denied, "friendly hint");
        assert!(mapped.to_string().contains("denied"));
        assert!(!mapped.to_string().contains("friendly hint"));
    }
}

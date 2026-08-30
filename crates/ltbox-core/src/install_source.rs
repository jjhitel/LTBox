//! Detects how the running LTBox executable was installed.
//!
//! Packages published by LTBox place a plain-text marker named
//! `ltbox.install-source` next to the executable. Its single line is one of
//! `scoop`, `winget`, `homebrew`, `deb`, or `rpm`. Parsing is ASCII
//! case-insensitive and ignores surrounding whitespace. Package-managed
//! locations remain package-managed without a valid marker, because a
//! third-party package must never be mistaken for a directly downloaded copy.
//!
//! `LTBOX_INSTALL_SOURCE` accepts the same values, plus `other` and `direct`,
//! and takes precedence over both the marker and executable location. This is
//! useful to packagers with an otherwise unrecognised layout as well as tests.

use std::path::{Path, PathBuf};

/// Filename installed next to the executable by LTBox-owned packages.
pub const INSTALL_SOURCE_MARKER_FILE: &str = "ltbox.install-source";

/// Environment override understood by [`install_source`].
pub const INSTALL_SOURCE_ENV: &str = "LTBOX_INSTALL_SOURCE";

/// Provenance of an LTBox installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstallSource {
    Scoop,
    WinGet,
    Homebrew,
    Deb,
    Rpm,
    /// A package-managed location whose exact manager is unknown.
    OtherPackageManager,
    Direct,
}

impl InstallSource {
    /// Whether the executable is owned by a package manager and must not
    /// overwrite itself.
    pub const fn is_package_managed(self) -> bool {
        !matches!(self, Self::Direct)
    }
}

/// Return the marker path associated with `executable_path`.
pub fn marker_path_for(executable_path: &Path) -> PathBuf {
    executable_path.with_file_name(INSTALL_SOURCE_MARKER_FILE)
}

/// Classify an install without consulting the process or filesystem.
///
/// `env_override` has highest priority. A recognised package marker identifies
/// the exact channel in any location. A `direct` marker cannot make a known
/// system-owned location self-updatable; only the explicit environment
/// override can force that result.
pub fn classify_install_source(
    executable_path: &Path,
    marker_contents: Option<&str>,
    env_override: Option<&str>,
) -> InstallSource {
    if let Some(source) = env_override.and_then(parse_source) {
        return source;
    }

    let package_managed_path = is_package_managed_path(executable_path);
    if let Some(source) = marker_contents.and_then(parse_source)
        && (source != InstallSource::Direct || !package_managed_path)
    {
        return source;
    }

    if package_managed_path {
        InstallSource::OtherPackageManager
    } else {
        InstallSource::Direct
    }
}

/// Detect the provenance of the running LTBox executable.
pub fn install_source() -> InstallSource {
    let env_override = std::env::var(INSTALL_SOURCE_ENV).ok();
    let Ok(executable_path) = std::env::current_exe() else {
        return classify_install_source(Path::new(""), None, env_override.as_deref());
    };
    let marker_contents = std::fs::read_to_string(marker_path_for(&executable_path)).ok();
    classify_install_source(
        &executable_path,
        marker_contents.as_deref(),
        env_override.as_deref(),
    )
}

fn parse_source(value: &str) -> Option<InstallSource> {
    match value.trim().to_ascii_lowercase().as_str() {
        "scoop" => Some(InstallSource::Scoop),
        "winget" => Some(InstallSource::WinGet),
        "homebrew" | "brew" => Some(InstallSource::Homebrew),
        "deb" => Some(InstallSource::Deb),
        "rpm" => Some(InstallSource::Rpm),
        "other" | "unknown" | "package-manager" => Some(InstallSource::OtherPackageManager),
        "direct" => Some(InstallSource::Direct),
        _ => None,
    }
}

fn is_package_managed_path(executable_path: &Path) -> bool {
    let path = executable_path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();

    // Scoop can be relocated, so recognise its manager-owned directory shape
    // instead of assuming a home-directory prefix.
    if path.contains("/apps/ltbox/") {
        return true;
    }

    // WinGet's package root is below LocalAppData, but matching its stable
    // suffix keeps this pure and does not require reading that environment.
    if path.contains("/microsoft/winget/packages/") {
        return true;
    }

    let path_without_drive = path
        .strip_prefix(|character: char| character.is_ascii_alphabetic())
        .and_then(|rest| rest.strip_prefix(':'))
        .unwrap_or(&path);

    [
        "/program files/",
        "/program files (x86)/",
        "/applications/",
        "/opt/homebrew/",
        "/usr/",
        "/opt/",
        "/snap/",
        "/nix/store/",
    ]
    .iter()
    .any(|prefix| path_without_drive.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognised_package_paths_are_managed_without_markers() {
        for path in [
            r"C:\Users\user\scoop\apps\ltbox\1.2.3\ltbox.exe",
            r"D:\PortableScoop\apps\ltbox\current\ltbox.exe",
            r"C:\Users\user\AppData\Local\Microsoft\WinGet\Packages\LTBox\ltbox.exe",
            r"C:\Program Files\LTBox\ltbox.exe",
            r"C:\Program Files (x86)\LTBox\ltbox.exe",
            "/Applications/LTBox.app/Contents/MacOS/ltbox",
            "/opt/homebrew/Caskroom/ltbox/1.2.3/LTBox.app/Contents/MacOS/ltbox",
            "/usr/local/Caskroom/ltbox/1.2.3/LTBox.app/Contents/MacOS/ltbox",
            "/usr/bin/ltbox",
            "/usr/local/bin/ltbox",
            "/opt/ltbox/ltbox",
            "/snap/ltbox/current/ltbox",
            "/nix/store/hash-ltbox/bin/ltbox",
        ] {
            let source = classify_install_source(Path::new(path), None, None);
            assert_eq!(
                source,
                InstallSource::OtherPackageManager,
                "{path} should be package-managed"
            );
            assert!(source.is_package_managed());
        }
    }

    #[test]
    fn markers_name_each_exact_package_channel() {
        for (marker, expected) in [
            ("scoop", InstallSource::Scoop),
            (" WinGet\r\n", InstallSource::WinGet),
            ("HOMEBREW\n", InstallSource::Homebrew),
            ("deb ", InstallSource::Deb),
            ("RpM", InstallSource::Rpm),
            ("other", InstallSource::OtherPackageManager),
        ] {
            assert_eq!(
                classify_install_source(Path::new("/usr/bin/ltbox"), Some(marker), None),
                expected
            );
        }
    }

    #[test]
    fn marker_in_non_system_path_still_identifies_package() {
        assert_eq!(
            classify_install_source(Path::new("/home/user/tools/ltbox"), Some("homebrew"), None),
            InstallSource::Homebrew
        );
        assert_eq!(
            classify_install_source(
                Path::new("/home/user/Downloads/ltbox"),
                Some("direct"),
                None,
            ),
            InstallSource::Direct
        );
    }

    #[test]
    fn unrecognised_marker_is_ignored() {
        assert_eq!(
            classify_install_source(
                Path::new("/home/user/Downloads/ltbox"),
                Some("future-manager"),
                None,
            ),
            InstallSource::Direct
        );
        assert_eq!(
            classify_install_source(Path::new("/usr/bin/ltbox"), Some("future-manager"), None),
            InstallSource::OtherPackageManager
        );
    }

    #[test]
    fn environment_override_can_force_channel_or_direct() {
        assert_eq!(
            classify_install_source(Path::new("/home/user/Downloads/ltbox"), None, Some(" RPM ")),
            InstallSource::Rpm
        );
        assert_eq!(
            classify_install_source(Path::new("/usr/bin/ltbox"), Some("deb"), Some("direct")),
            InstallSource::Direct
        );
    }

    #[test]
    fn direct_marker_cannot_make_system_path_self_updatable() {
        assert_eq!(
            classify_install_source(Path::new("/usr/bin/ltbox"), Some("direct"), None),
            InstallSource::OtherPackageManager
        );
    }

    #[test]
    fn ordinary_user_path_is_direct() {
        assert_eq!(
            classify_install_source(Path::new("/home/user/Downloads/ltbox"), None, None),
            InstallSource::Direct
        );
        assert_eq!(
            classify_install_source(Path::new(r"C:\Users\user\Downloads\ltbox.exe"), None, None),
            InstallSource::Direct
        );
    }

    #[test]
    fn marker_path_is_next_to_executable() {
        assert_eq!(
            marker_path_for(Path::new("/usr/bin/ltbox")),
            PathBuf::from("/usr/bin/ltbox.install-source")
        );
    }
}

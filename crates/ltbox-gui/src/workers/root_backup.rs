//! Root-backup contents shared by the root and unroot workers.

use std::path::Path;

use crate::UnrootType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BackupRootTarget {
    Boot,
    InitBoot,
}

impl BackupRootTarget {
    pub(super) const fn partition_base(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::InitBoot => "init_boot",
        }
    }

    pub(super) const fn filename(self) -> &'static str {
        match self {
            Self::Boot => "boot.img",
            Self::InitBoot => "init_boot.img",
        }
    }
}

/// What a backup folder holds, and therefore what Unroot must restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BackupContents {
    pub(super) root_target: BackupRootTarget,
    /// `true` when this model's root flow changes vbmeta and its image is present.
    pub(super) restore_vbmeta: bool,
}

/// Resolve the images from their on-disk names.
///
/// The optional backup manifest is informational metadata. It is deliberately
/// not parsed here: an old, malformed, or mismatching manifest must not block
/// installation of otherwise valid stock images. The filename convention,
/// selected unroot route, and model's vbmeta participation determine the files.
pub(super) fn resolve_backup_contents(
    backup_dir: &Path,
    unroot_type: UnrootType,
    device_model: &str,
) -> Result<BackupContents, String> {
    // Legacy Magisk LKM backups can contain either target. Prefer boot when it
    // is present, retaining the historical init_boot missing-file diagnostic
    // when neither candidate exists. APatch GKI always restores boot.
    let root_target = match unroot_type {
        UnrootType::MagiskLkm => {
            if backup_dir.join(BackupRootTarget::Boot.filename()).is_file() {
                BackupRootTarget::Boot
            } else {
                BackupRootTarget::InitBoot
            }
        }
        UnrootType::APatchGki => BackupRootTarget::Boot,
    };
    let target = match root_target {
        BackupRootTarget::Boot => ltbox_patch::root_pipeline::RootImageTarget::Boot,
        BackupRootTarget::InitBoot => ltbox_patch::root_pipeline::RootImageTarget::InitBoot,
    };
    // Old fixed backup folders can retain vbmeta from another run. Apply the
    // same participation rule as Root instead of trusting its mere presence
    // or consuming the old manifest. Chained boot and TB323FU leave it alone.
    let restore_vbmeta = ltbox_patch::root_pipeline::root_run_rebuilds_vbmeta(target, device_model)
        && !crate::root_skips_avb_postprocess(device_model)
        && backup_dir.join("vbmeta.img").is_file();
    Ok(BackupContents {
        root_target,
        restore_vbmeta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_manifest_does_not_block_boot_selection() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("boot.img"), []).unwrap();
        std::fs::write(temp.path().join("root-backup.json"), b"not json").unwrap();
        std::fs::write(temp.path().join("manifest.json"), b"not json").unwrap();

        let contents =
            resolve_backup_contents(temp.path(), UnrootType::MagiskLkm, "TB320FC").unwrap();
        assert_eq!(contents.root_target, BackupRootTarget::Boot);
        assert!(!contents.restore_vbmeta);
    }

    #[test]
    fn mismatching_manifest_does_not_change_init_boot_selection() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("init_boot.img"), []).unwrap();
        std::fs::write(
            temp.path().join("root-backup.json"),
            br#"{"root_partition":"boot","vbmeta":true}"#,
        )
        .unwrap();
        std::fs::write(
            temp.path().join("manifest.json"),
            br#"{"model":"other device","fingerprint":"other build","files":[{"filename":"boot.img","sha256":"wrong","rollback_index":999}]}"#,
        )
        .unwrap();

        let contents =
            resolve_backup_contents(temp.path(), UnrootType::MagiskLkm, "TB320FC").unwrap();
        assert_eq!(contents.root_target, BackupRootTarget::InitBoot);
        assert!(!contents.restore_vbmeta);
    }

    #[test]
    fn manifest_free_magisk_backup_infers_init_boot_filename() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("init_boot.img"), []).unwrap();

        assert_eq!(
            resolve_backup_contents(temp.path(), UnrootType::MagiskLkm, "TB320FC")
                .unwrap()
                .root_target,
            BackupRootTarget::InitBoot
        );
    }

    #[test]
    fn manifest_free_magisk_backup_infers_boot_filename() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("boot.img"), []).unwrap();

        assert_eq!(
            resolve_backup_contents(temp.path(), UnrootType::MagiskLkm, "TB320FC")
                .unwrap()
                .root_target,
            BackupRootTarget::Boot
        );
    }

    #[test]
    fn vbmeta_presence_controls_restore_without_manifest() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("boot.img"), []).unwrap();
        std::fs::write(temp.path().join("vbmeta.img"), []).unwrap();

        let contents =
            resolve_backup_contents(temp.path(), UnrootType::MagiskLkm, "TB320FC").unwrap();
        assert_eq!(contents.root_target, BackupRootTarget::Boot);
        assert!(contents.restore_vbmeta);
    }

    #[test]
    fn stale_legacy_vbmeta_is_ignored_when_root_leaves_it_untouched() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("boot.img"), b"current root image").unwrap();
        std::fs::write(temp.path().join("vbmeta.img"), b"stale from another run").unwrap();
        for model in ["TB322FC", "TB323FU", "TB321FU"] {
            let contents =
                resolve_backup_contents(temp.path(), UnrootType::APatchGki, model).unwrap();
            assert_eq!(contents.root_target, BackupRootTarget::Boot);
            assert!(!contents.restore_vbmeta, "{model}");
        }
        for model in ["TB320FC", "LAVIETab9QHD1"] {
            assert!(
                resolve_backup_contents(temp.path(), UnrootType::APatchGki, model)
                    .unwrap()
                    .restore_vbmeta
            );
        }
    }

    #[test]
    fn init_boot_restores_vbmeta_except_on_efisp_root_route() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("init_boot.img"), b"root image").unwrap();
        std::fs::write(temp.path().join("vbmeta.img"), b"vbmeta").unwrap();
        assert!(
            resolve_backup_contents(temp.path(), UnrootType::MagiskLkm, "TB322FC")
                .unwrap()
                .restore_vbmeta
        );
        assert!(
            !resolve_backup_contents(temp.path(), UnrootType::MagiskLkm, "TB323FU")
                .unwrap()
                .restore_vbmeta
        );
    }
}

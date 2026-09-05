//! TB323FU efisp ARB-overlay provisioning: pick the efisp asset variant
//! and build the per-LUN overlay set. Extracted from main.rs.

use crate::*;

/// GBL EFI asset suffix for a TB323FU target firmware, by region (`is_prc`) and
/// whether the anti-rollback build is needed (`arb`). Picks the
/// `*_prc.efi` / `*_row.efi` asset (or `*_prc_arb.efi` / `*_row_arb.efi`) from
/// the gbl_root_baldur release. The `_arb` GBL roots trust at the testkey so it
/// accepts the testkey-re-signed AVB chain LTBox stages on a downgrade. The
/// region comes from the vendor_boot `product_region` DTB marker — TB323FU's AVB
/// fingerprint carries no `_PRC`/`_ROW` token.
pub(crate) fn efisp_asset_suffix(is_prc: bool, arb: bool) -> &'static str {
    match (is_prc, arb) {
        (true, false) => "_prc.efi",
        (true, true) => "_prc_arb.efi",
        (false, false) => "_row.efi",
        (false, true) => "_row_arb.efi",
    }
}

/// A dumped `efisp` partition counts as empty (un-provisioned) when every byte
/// is zero and the dump is nonempty — the stock/erased state.
/// A GBL-provisioned efisp carries the EFI
/// payload, so it has non-zero bytes. The TB323FU root flow uses an empty result
/// to provision the appropriate region GBL before continuing.
pub(crate) fn efisp_is_empty(data: &[u8]) -> bool {
    !data.is_empty() && data.iter().all(|&b| b == 0)
}

/// A missing, unreadable or zero-length dump cannot establish provisioning state.
fn read_efisp_is_empty(path: &std::path::Path) -> Result<bool, String> {
    let data = std::fs::read(path).map_err(|error| {
        tr_args!(
            "err_efisp_read_failed",
            path = path.display(),
            error = error
        )
    })?;
    if data.is_empty() {
        return Err(ltbox_core::i18n::tr("err_efisp_dump_empty"));
    }
    Ok(efisp_is_empty(&data))
}

/// Select a GBL only after positively identifying the vendor_boot region.
pub(crate) fn efisp_suffix_for_vendor_boot(
    path: &std::path::Path,
    arb: bool,
) -> Result<&'static str, String> {
    use ltbox_patch::region::RegionTarget;
    match ltbox_patch::region::detect_product_region(path) {
        Some(RegionTarget::Prc) => Ok(efisp_asset_suffix(true, arb)),
        Some(RegionTarget::Row) => Ok(efisp_asset_suffix(false, arb)),
        None => Err(tr_args!("err_efisp_region_unknown", path = path.display())),
    }
}

/// Inspect TB323FU `efisp` and, when it is still all-zero, stage the matching
/// region GBL used by the Root and KonaBess device workers. The caller performs
/// the actual efisp write with [`provision_tb323fu_efisp`] at the safest point
/// in its own operation.
pub(crate) fn prepare_tb323fu_efisp(
    session: &mut ltbox_device::edl::EdlSession,
    slot_suffix: &str,
    dumped_vendor_boot: Option<&std::path::Path>,
    work_dir: &std::path::Path,
    efi_dir: &std::path::Path,
    log: &mut Vec<String>,
) -> std::result::Result<Option<std::path::PathBuf>, String> {
    live!(
        log,
        "[Root] {}",
        ltbox_core::i18n::tr("log_root_efisp_check")
    );
    let dumped_efisp = work_dir.join("efisp.img");
    let efisp_lun = ltbox_core::partition_lun::lun_for_partition("efisp").unwrap_or(4);
    session
        .dump_partition("efisp", &dumped_efisp, 0, efisp_lun, log)
        .map_err(|e| {
            tr_args!(
                "err_root_dump_partition_failed",
                partition = "efisp",
                error = e
            )
        })?;
    let efisp_empty = read_efisp_is_empty(&dumped_efisp)?;
    if !efisp_empty {
        live!(log, "[Root] {}", ltbox_core::i18n::tr("log_root_efisp_ok"));
        return Ok(None);
    }

    // Empty efisp is the stock, GBL-unprovisioned state. Region comes from
    // vendor_boot's product_region marker because the AVB fingerprint carries
    // no PRC/ROW token on TB323FU.
    let suffix = if let Some(path) = dumped_vendor_boot {
        efisp_suffix_for_vendor_boot(path, false)?
    } else {
        let partition = format!("vendor_boot{slot_suffix}");
        let path = work_dir.join("vendor_boot.img");
        let lun = ltbox_core::partition_lun::lun_for_partition(&partition)
            .ok_or_else(|| tr_args!("err_no_hardcoded_lun", partition = partition))?;
        session
            .dump_partition(&partition, &path, 0, lun, log)
            .map_err(|error| {
                tr_args!(
                    "err_root_dump_partition_failed",
                    partition = partition,
                    error = error
                )
            })?;
        efisp_suffix_for_vendor_boot(&path, false)?
    };
    fetch_efisp_asset(suffix, efi_dir, "[Root]", log).map(Some)
}

/// Download the pinned efisp GBL for `suffix` into `efi_dir` and verify it.
///
/// Every caller writes the result to the device's `efisp` partition, so the
/// release tag is pinned, the asset name must be one of a fixed set, and the
/// bytes are SHA-256 checked before the path is handed back. The Flash path
/// grew those three guards first; Root and KonaBess reached the same partition
/// through a copy that floated on `latest` and verified nothing.
pub(crate) fn fetch_efisp_asset(
    suffix: &str,
    efi_dir: &std::path::Path,
    log_tag: &str,
    log: &mut Vec<String>,
) -> std::result::Result<std::path::PathBuf, String> {
    let expected_name = efisp_expected_asset(suffix).ok_or_else(|| {
        tr_args!(
            "err_efisp_asset_unknown",
            asset = suffix,
            tag = EFISP_GBL_RELEASE_TAG
        )
    })?;
    live!(
        log,
        "{} {}",
        log_tag,
        tr_args!("live_flash_efisp_fetch", variant = suffix)
    );
    let gh = ltbox_core::github::GitHubClient::from_url("github.com/miner7222/gbl_root_baldur")
        .map_err(|e| tr_args!("err_efisp_github_failed", error = e))?;
    let assets = gh
        .release_by_tag(EFISP_GBL_RELEASE_TAG)
        .map_err(|e| tr_args!("err_efisp_github_failed", error = e))?;
    let (asset_name, asset_url) = assets
        .into_iter()
        .find(|(name, _)| name == expected_name)
        .ok_or_else(|| {
            tr_args!(
                "err_efisp_asset_missing",
                suffix = suffix,
                tag = EFISP_GBL_RELEASE_TAG
            )
        })?;
    let _ = std::fs::remove_dir_all(efi_dir);
    std::fs::create_dir_all(efi_dir)
        .map_err(|e| tr_args!("err_efisp_work_dir_failed", error = e))?;
    let efi_path = efi_dir.join(&asset_name);
    ltbox_core::downloader::download_to_file(&asset_url, &efi_path, log)
        .map_err(|e| tr_args!("err_efisp_download_failed", asset = asset_name, error = e))?;
    verify_efisp_asset(&efi_path, &asset_name)?;
    live!(
        log,
        "{} {}",
        log_tag,
        tr_args!("live_flash_efisp_fetched", name = asset_name)
    );
    Ok(efi_path)
}

/// Pinned gbl_root_baldur release used for TB323FU efisp GBL images.
pub(crate) const EFISP_GBL_RELEASE_TAG: &str = "5.3.120-mod5";

/// Exact asset names accepted for the pinned efisp release.
pub(crate) const EFISP_EXPECTED_ASSETS: &[(&str, &str)] = &[
    (
        "generic_superfastboot_prc.efi",
        "22471c543e13a433cb05c5c54bcfaf107f2643a6cfd7e46d8d73764b349e8cf2",
    ),
    (
        "generic_superfastboot_prc_arb.efi",
        "fc55e6a4912f20c1bf0c664bb985e10d0c4e0944f92fab5e4f855b22e2aefcdf",
    ),
    (
        "generic_superfastboot_row.efi",
        "90b16cacc4f2f6aded2c5bf0eed7d20b6a294115f6cf09dab555b4a4496e2628",
    ),
    (
        "generic_superfastboot_row_arb.efi",
        "99b62f4aeca619df4f480f6bffa3f0fea3ef2516a8fe48a092613c2aa68d10e2",
    ),
];

/// Map a region/ARB suffix to the exact pinned asset name for the
/// `5.3.120-mod5` gbl_root_baldur release. Unknown suffixes refuse.
pub(crate) fn efisp_expected_asset(suffix: &str) -> Option<&'static str> {
    match suffix {
        "_prc.efi" => Some("generic_superfastboot_prc.efi"),
        "_prc_arb.efi" => Some("generic_superfastboot_prc_arb.efi"),
        "_row.efi" => Some("generic_superfastboot_row.efi"),
        "_row_arb.efi" => Some("generic_superfastboot_row_arb.efi"),
        _ => None,
    }
}

/// SHA-256 hex for a pinned efisp asset name. Unknown names refuse.
pub(crate) fn efisp_expected_sha256(asset_name: &str) -> Option<&'static str> {
    EFISP_EXPECTED_ASSETS
        .iter()
        .find(|(name, _)| *name == asset_name)
        .map(|(_, hash)| *hash)
}

pub(crate) fn sha256_hex_file(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Verify a downloaded efisp EFI against the pinned name → SHA-256 map.
/// Unknown names and hash mismatches both refuse.
pub(crate) fn verify_efisp_asset(path: &std::path::Path, asset_name: &str) -> Result<(), String> {
    let expected = efisp_expected_sha256(asset_name).ok_or_else(|| {
        tr_args!(
            "err_efisp_asset_unknown",
            asset = asset_name,
            tag = EFISP_GBL_RELEASE_TAG
        )
    })?;
    let actual = sha256_hex_file(path)?;
    if actual != expected {
        return Err(tr_args!(
            "err_efisp_hash_mismatch",
            asset = asset_name,
            expected = expected,
            actual = actual
        ));
    }
    Ok(())
}

/// Provision a staged TB323FU region GBL. `None` is the already-provisioned
/// path and deliberately performs no device write.
pub(crate) fn provision_tb323fu_efisp(
    session: &mut ltbox_device::edl::EdlSession,
    efi: Option<&std::path::Path>,
    log: &mut Vec<String>,
) -> std::result::Result<bool, String> {
    let Some(efi) = efi else {
        return Ok(false);
    };
    let efisp_lun = ltbox_core::partition_lun::lun_for_partition("efisp").unwrap_or(4);
    live!(
        log,
        "[Root] {}",
        ltbox_core::i18n::tr("live_flash_efisp_flash")
    );
    session
        .flash_partition("efisp", efi, 0, efisp_lun, log)
        .map_err(|e| tr_args!("err_root_efisp_provision_failed", error = e))?;
    live!(
        log,
        "[Root] {}",
        ltbox_core::i18n::tr("live_flash_efisp_flashed")
    );
    Ok(true)
}

/// One staged ARB overlay: (GPT label, UFS LUN, patched image path).
pub(crate) type ArbOverlay = (String, u8, std::path::PathBuf);

/// Testkey re-sign overlays for an AVB flash — used by the TB323FU anti-rollback
/// path and the non-TB323FU Lenovo-key / cross-region re-sign. The device-committed
/// boot + vbmeta_system indices come from `device_floors` (component-wise across
/// both slots on EDL-start) or are read from the active slot here.
///
/// Layout-aware: it re-signs exactly the partitions the (base) vbmeta chains —
/// each needs a matching `<part>.img` — so packages without recovery (or with a
/// different set of chained partitions) work too. boot / vbmeta_system bump to
/// the device
/// floor (`max`, never lowered); other chained partitions (e.g. recovery) are
/// re-signed only; the vbmeta is rebuilt with selected chain partition descriptor
/// public keys updated to the testkey and flashed LAST (it ties the chain
/// together — shrinks the partial-write brick window). For a Lenovo-key cross-region
/// install, `vbmeta_base` overrides the rebuild base with the region-converted,
/// testkey vbmeta so its recomputed vendor_boot hash is preserved; otherwise it
/// uses the firmware's own `vbmeta.img`.
///
/// `force_resign` re-signs even without a downgrade (Lenovo-key firmware on a testkey
/// device). Returns `(overlays, need)`; `need` is the downgrade flag — the
/// TB323FU caller swaps the efisp GBL to its `_arb` (testkey-root) variant.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_tb323fu_arb_overlays(
    session: &mut ltbox_device::edl::EdlSession,
    fw_dir: &std::path::Path,
    work_dir: &std::path::Path,
    slot: Option<&str>,
    device_floors: Option<(u64, u64)>,
    force_resign: bool,
    vbmeta_base: Option<&std::path::Path>,
    log: &mut Vec<String>,
) -> std::result::Result<(Vec<ArbOverlay>, bool), String> {
    const KEY: &str = "testkey_rsa4096";
    const ALGO: &str = "SHA256_RSA4096";

    let lun_of = |label: &str| -> std::result::Result<u8, String> {
        ltbox_core::partition_lun::lun_for_partition(label)
            .ok_or_else(|| format!("no hardcoded LUN for {label}"))
    };
    let idx_of = |path: &std::path::Path| -> std::result::Result<u64, String> {
        Ok(ltbox_patch::avb::extract_image_avb_info(path)
            .map_err(|e| format!("AVB inspect {}: {e}", path.display()))?
            .rollback_index)
    };

    // 1. Inspect base vbmeta (caller override for cross-region, else firmware's)
    //    and the partitions it chains. Re-sign only the ones we can handle and
    //    update their chain partition descriptor public keys: a plain partition
    //    name, an install image, and a resolvable A/B GPT label/LUN. Other
    //    chained partitions (e.g. vbmeta_vendor) keep their stock chain partition
    //    descriptor + stock image.
    let inst_vbmeta = vbmeta_base
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| fw_dir.join("vbmeta.img"));
    if !inst_vbmeta.exists() {
        return Err(format!("install image missing: {}", inst_vbmeta.display()));
    }
    let chain_descriptors = ltbox_patch::avb::chain_partition_descriptors(&inst_vbmeta)
        .map_err(|e| format!("vbmeta chain partition descriptors: {e}"))?;
    let inst_img = |p: &str| fw_dir.join(format!("{p}.img"));
    let has = |name: &str| chain_descriptors.iter().any(|c| c.name == name);
    // GPT label for a chained partition: `_a` for A/B, the unsuffixed name for a
    // `DO_NOT_USE_AB` chain (AVB verifies the unsuffixed partition).
    let label_of = |c: &ltbox_patch::avb::ChainPartitionDescriptor| -> String {
        if c.do_not_use_ab {
            c.name.clone()
        } else {
            format!("{}_a", c.name)
        }
    };
    // The floor read + index bump below assume A/B slots for the rollback-
    // protected partitions, so reject a non-A/B boot / vbmeta_system layout.
    if chain_descriptors
        .iter()
        .any(|c| (c.name == "boot" || c.name == "vbmeta_system") && c.do_not_use_ab)
    {
        return Err("non-A/B boot/vbmeta_system rollback layout is unsupported".to_string());
    }
    // 2. Device-committed per-location indices (boot + vbmeta_system). On an
    //    EDL-start flash the caller passes component-wise maxima already read
    //    across BOTH slots; otherwise read the ACTIVE slot here (a first-time
    //    user may still be on `_b`, so don't assume `_a`).
    let (dev_boot_idx, dev_vbs_idx) = match device_floors {
        Some(floors) => floors,
        None => {
            let dev_boot = format!("boot{}", active_slot_suffix(slot));
            let dev_vbs = format!("vbmeta_system{}", active_slot_suffix(slot));
            let dev_boot_img = work_dir.join(format!("dev_{dev_boot}.img"));
            let dev_vbs_img = work_dir.join(format!("dev_{dev_vbs}.img"));
            session
                .dump_partition(&dev_boot, &dev_boot_img, 0, lun_of(&dev_boot)?, log)
                .map_err(|e| format!("dump device {dev_boot}: {e}"))?;
            session
                .dump_partition(&dev_vbs, &dev_vbs_img, 0, lun_of(&dev_vbs)?, log)
                .map_err(|e| format!("dump device {dev_vbs}: {e}"))?;
            let b = idx_of(&dev_boot_img)?;
            let v = idx_of(&dev_vbs_img)?;
            let _ = std::fs::remove_file(&dev_boot_img);
            let _ = std::fs::remove_file(&dev_vbs_img);
            (b, v)
        }
    };

    // 3. Rollback-protected install indices (boot + vbmeta_system when chained).
    let inst_boot_idx = if has("boot") {
        idx_of(&inst_img("boot"))?
    } else {
        0
    };
    let inst_vbs_idx = if has("vbmeta_system") {
        idx_of(&inst_img("vbmeta_system"))?
    } else {
        0
    };
    ltbox_core::live!(
        log,
        "[ARB] {}",
        tr_args!(
            "live_arb_tb323_indices",
            boot_i = inst_boot_idx.to_string(),
            boot_d = dev_boot_idx.to_string(),
            vbs_i = inst_vbs_idx.to_string(),
            vbs_d = dev_vbs_idx.to_string()
        )
    );

    // 4. need = a rollback-protected install image is behind the device index.
    let need = (has("boot") && inst_boot_idx < dev_boot_idx)
        || (has("vbmeta_system") && inst_vbs_idx < dev_vbs_idx);
    if !need && !force_resign {
        ltbox_core::live!(
            log,
            "[ARB] {}",
            ltbox_core::i18n::tr("live_arb_tb323_skip_uptodate")
        );
        return Ok((Vec::new(), false));
    }

    // 5. A re-sign is needed: validate + select the chained partitions to
    //    re-sign. Two distinct failure modes:
    //   * Missing image — the firmware chains the partition but ships no
    //     `<part>.img`, so the testkey root would delegate to a partition the
    //     package may never flash. Abort: a stale child fails AVB verification.
    //   * Image present but the LUN is not in the static map (e.g. vbmeta_vendor)
    //     — no overlay can be staged, so leave it stock: rawprogram still flashes
    //     the firmware's own image, which matches its preserved (firmware-key)
    //     chain descriptor under the testkey-signed root.
    // The rollback-protected boot / vbmeta_system MUST be re-signable.
    let mut to_resign: Vec<&ltbox_patch::avb::ChainPartitionDescriptor> = Vec::new();
    for c in &chain_descriptors {
        if c.name.is_empty()
            || !c
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(format!(
                "unsafe chain partition name in vbmeta: {:?}",
                c.name
            ));
        }
        if !inst_img(&c.name).exists() {
            return Err(format!(
                "vbmeta chains {} but its install image is missing: {}",
                c.name,
                inst_img(&c.name).display()
            ));
        }
        if ltbox_core::partition_lun::lun_for_partition(&label_of(c)).is_some() {
            to_resign.push(c);
        } else if c.name == "boot" || c.name == "vbmeta_system" {
            return Err(format!(
                "rollback-protected {} is chained but its LUN is unknown",
                c.name
            ));
        }
    }

    // 6. Re-sign each handled chained partition to the testkey. boot /
    //    vbmeta_system bump to the device floor (never lower the image's own
    //    claim, hence max()); others are re-signed only. Flash boot LAST among
    //    them (just before vbmeta_a) to shrink the partial-write brick window.
    let boot_target = inst_boot_idx.max(dev_boot_idx);
    let vbs_target = inst_vbs_idx.max(dev_vbs_idx);
    // Re-sign with the testkey's OWN algorithm (derived from KEY), not the source
    // image's: a Lenovo-key image may use a different RSA size, which avbtool would
    // reject against the testkey.
    let key_algo = ltbox_patch::avb::algorithm_for_key_spec(KEY)
        .ok_or_else(|| format!("unknown AVB algorithm for {KEY}"))?;
    let mut ordered = to_resign.clone();
    ordered.sort_by_key(|c| u8::from(c.name == "boot"));
    let mut overlays: Vec<ArbOverlay> = Vec::new();
    for &c in &ordered {
        let name = c.name.as_str();
        let out = work_dir.join(format!("{name}.arb.img"));
        std::fs::copy(inst_img(name), &out).map_err(|e| format!("copy {name}: {e}"))?;
        let target = match name {
            "boot" => Some(boot_target),
            "vbmeta_system" => Some(vbs_target),
            _ => None,
        };
        ltbox_patch::avb::resign_image(&out, KEY, &key_algo, target)
            .map_err(|e| format!("resign {name}: {e}"))?;
        let label = label_of(c);
        let lun = lun_of(&label)?;
        overlays.push((label, lun, out));
    }

    // 7. Rebuild vbmeta on the base, updating chain partition descriptor public
    //    keys for the re-signed chained partitions to the testkey (others keep
    //    their stock pubkey); flash it LAST.
    let out_vbmeta = work_dir.join("vbmeta.arb.img");
    let chain_partition_names: Vec<&str> = to_resign.iter().map(|c| c.name.as_str()).collect();
    ltbox_patch::avb::rebuild_vbmeta_with_chain_key_overrides(
        &out_vbmeta,
        &inst_vbmeta,
        &chain_partition_names,
        KEY,
        KEY,
        ALGO,
    )
    .map_err(|e| format!("rebuild vbmeta: {e}"))?;
    overlays.push(("vbmeta_a".to_string(), lun_of("vbmeta_a")?, out_vbmeta));
    ltbox_core::live!(
        log,
        "[ARB] {}",
        tr_args!(
            "live_arb_tb323_resigned",
            boot = boot_target.to_string(),
            vbs = vbs_target.to_string()
        )
    );

    Ok((overlays, need))
}

#[cfg(test)]
mod provisioning_tests {
    use super::{efisp_suffix_for_vendor_boot, read_efisp_is_empty};

    #[test]
    fn efisp_dump_requires_readable_nonempty_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("efisp.img");
        assert!(read_efisp_is_empty(&path).is_err());
        assert!(read_efisp_is_empty(dir.path()).is_err());
        std::fs::write(&path, []).unwrap();
        assert!(read_efisp_is_empty(&path).is_err());
        std::fs::write(&path, [0; 4096]).unwrap();
        assert!(read_efisp_is_empty(&path).unwrap());
        std::fs::write(&path, [0, 0, 1, 0]).unwrap();
        assert!(!read_efisp_is_empty(&path).unwrap());
    }

    #[test]
    fn gbl_selection_requires_a_known_region_for_stock_and_arb() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vendor_boot.img");
        for arb in [false, true] {
            assert!(efisp_suffix_for_vendor_boot(&path, arb).is_err());
            assert!(efisp_suffix_for_vendor_boot(dir.path(), arb).is_err());
        }
        for data in [b"".as_slice(), b"invalid", b"product_region\0model\0"] {
            std::fs::write(&path, data).unwrap();
            for arb in [false, true] {
                assert!(efisp_suffix_for_vendor_boot(&path, arb).is_err());
            }
        }
        // Minimal product_region FDT property layout accepted by the detector.
        for (region, stock, arb) in [
            (b"PRC\0", "_prc.efi", "_prc_arb.efi"),
            (b"ROW\0", "_row.efi", "_row_arb.efi"),
        ] {
            let mut data = b"product_region\0\0".to_vec();
            data.extend_from_slice(&[0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0x26, 0xb7]);
            data.extend_from_slice(region);
            std::fs::write(&path, data).unwrap();
            assert_eq!(efisp_suffix_for_vendor_boot(&path, false).unwrap(), stock);
            assert_eq!(efisp_suffix_for_vendor_boot(&path, true).unwrap(), arb);
        }
    }
}

//! End-to-end regression coverage for the pure ARB overlay builder.
//!
//! These fixtures deliberately use the embedded avbtool-rs test keys.  They
//! exercise the same chain descriptors and hash footers as a firmware package,
//! while keeping the tests offline, small, and independent of a device.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::{TempDir, tempdir};

use super::arb_overlay::{ArbOverlay, build_testkey_arb_overlays_for_floors};

const SOURCE_KEY: &str = "testkey_rsa2048";
const SOURCE_ALGORITHM: &str = "SHA256_RSA2048";
const OUTPUT_KEY: &str = "testkey_rsa4096";
const OUTPUT_ALGORITHM: &str = "SHA256_RSA4096";
const OUTPUT_KEY_SHA1: &str = "2597c218aae470a130f61162feaae70afd97f011";

const BOOT_INDEX: u64 = 10;
const VBMETA_SYSTEM_INDEX: u64 = 20;

struct Fixture {
    _temp: TempDir,
    firmware: PathBuf,
    work: PathBuf,
    region_vbmeta: PathBuf,
    original: BTreeMap<&'static str, Vec<u8>>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempdir().expect("fixture tempdir");
        let firmware = temp.path().join("firmware");
        let work = temp.path().join("work");
        fs::create_dir_all(&firmware).expect("firmware directory");
        fs::create_dir_all(&work).expect("work directory");

        make_hash_image(&firmware.join("boot.img"), "boot", BOOT_INDEX, 0, 0x11);
        make_hash_image(
            &firmware.join("vbmeta_system.img"),
            "vbmeta_system",
            VBMETA_SYSTEM_INDEX,
            1,
            0x22,
        );
        // This descriptor is not chained.  It stands in for a region-converted
        // vbmeta base whose unrelated vendor_boot hash must survive a rebuild.
        make_hash_image(&firmware.join("vendor_boot.img"), "vendor_boot", 7, 3, 0x33);

        let boot_key =
            avbtool_rs::crypto::extract_public_key(SOURCE_KEY).expect("source boot public key");
        let vbmeta = firmware.join("vbmeta.img");
        make_root_vbmeta(&vbmeta, &firmware.join("vendor_boot.img"), &boot_key, false);
        let region_vbmeta = firmware.join("vbmeta_region.img");
        make_root_vbmeta(
            &region_vbmeta,
            &firmware.join("vendor_boot.img"),
            &boot_key,
            true,
        );

        let original = [
            "boot",
            "vbmeta_system",
            "vendor_boot",
            "vbmeta",
            "vbmeta_region",
        ]
        .into_iter()
        .map(|name| {
            (
                name,
                fs::read(firmware.join(format!("{name}.img"))).expect("fixture image"),
            )
        })
        .collect();

        Self {
            _temp: temp,
            firmware,
            work,
            region_vbmeta,
            original,
        }
    }

    fn fresh_work(&self, name: &str) -> PathBuf {
        let work = self.work.join(name);
        fs::create_dir_all(&work).expect("test work directory");
        work
    }

    fn assert_originals_unchanged(&self) {
        for (name, expected) in &self.original {
            let actual = fs::read(self.firmware.join(format!("{name}.img")))
                .expect("source fixture still exists");
            assert_eq!(actual, *expected, "source image {name} was modified");
        }
    }
}

fn make_hash_image(path: &Path, partition_name: &str, rollback: u64, location: u32, fill: u8) {
    fs::write(path, vec![fill; 8 * 1024]).expect("raw fixture image");
    avbtool_rs::footer::add_hash_footer(
        path,
        &avbtool_rs::footer::HashFooterArgs {
            partition_size: Some(128 * 1024),
            dynamic_partition_size: false,
            partition_name: partition_name.to_string(),
            hash_algorithm: "sha256".to_string(),
            salt: Some(vec![fill; 32]),
            chain_partitions: Vec::new(),
            algorithm_name: SOURCE_ALGORITHM.to_string(),
            key_spec: Some(SOURCE_KEY.to_string()),
            public_key_metadata: None,
            rollback_index: rollback,
            flags: 0,
            rollback_index_location: location,
            properties: Vec::new(),
            kernel_cmdlines: Vec::new(),
            include_descriptors_from_images: Vec::new(),
            release_string: None,
            append_to_release_string: None,
            output_vbmeta_image: None,
            do_not_append_vbmeta_image: false,
            use_persistent_digest: false,
            do_not_use_ab: false,
        },
    )
    .expect("source AVB footer");
}

fn make_root_vbmeta(
    path: &Path,
    vendor_boot: &Path,
    source_public_key: &[u8],
    region_variant: bool,
) {
    let args = avbtool_rs::builder::VbmetaImageArgs {
        algorithm_name: SOURCE_ALGORITHM.to_string(),
        key_spec: Some(SOURCE_KEY.to_string()),
        public_key_metadata: None,
        rollback_index: if region_variant { 12 } else { 11 },
        flags: 0,
        rollback_index_location: 0,
        properties: vec![avbtool_rs::builder::PropertySpec {
            key: "com.android.build.system.fingerprint".to_string(),
            value: if region_variant {
                b"fixture/region".to_vec()
            } else {
                b"fixture/stock".to_vec()
            },
        }],
        kernel_cmdlines: Vec::new(),
        extra_descriptors: Vec::new(),
        include_descriptors_from_images: vec![vendor_boot.to_path_buf()],
        chain_partitions: vec![
            avbtool_rs::builder::ChainPartitionSpec {
                partition_name: "boot".to_string(),
                rollback_index_location: 0,
                public_key: source_public_key.to_vec(),
                flags: 0,
            },
            avbtool_rs::builder::ChainPartitionSpec {
                partition_name: "vbmeta_system".to_string(),
                rollback_index_location: 1,
                public_key: source_public_key.to_vec(),
                flags: 0,
            },
        ],
        release_string: Some("ltbox-manual-rollback-fixture".to_string()),
        append_to_release_string: None,
        padding_size: 4096,
    };
    avbtool_rs::builder::make_vbmeta_image(path, &args).expect("source root vbmeta");
}

fn call_builder(
    fixture: &Fixture,
    work: &Path,
    floors: (u64, u64),
    manual: Option<ltbox_patch::rollback::RollbackIndices>,
    force_resign: bool,
    vbmeta_base: Option<&Path>,
) -> (Vec<ArbOverlay>, bool) {
    let mut log = Vec::new();
    build_testkey_arb_overlays_for_floors(
        &fixture.firmware,
        work,
        floors,
        manual,
        force_resign,
        vbmeta_base,
        &mut log,
    )
    .unwrap_or_else(|error| panic!("overlay builder failed: {error}"))
}

fn overlay<'a>(overlays: &'a [ArbOverlay], filename: &str) -> &'a Path {
    overlays
        .iter()
        .find_map(|(_, _, path)| {
            (path.file_name()?.to_str()? == filename).then_some(path.as_path())
        })
        .unwrap_or_else(|| panic!("missing overlay {filename}: {overlays:?}"))
}

fn assert_testkey_image(path: &Path, expected_index: u64, expected_location: u32) {
    let info = ltbox_patch::avb::extract_image_avb_info(path).expect("overlay AVB metadata");
    assert_eq!(info.algorithm, OUTPUT_ALGORITHM, "{}", path.display());
    assert_eq!(info.rollback_index, expected_index, "{}", path.display());
    assert_eq!(
        info.rollback_index_location,
        expected_location,
        "{}",
        path.display()
    );
    assert_eq!(
        info.public_key_sha1,
        Some(OUTPUT_KEY_SHA1.to_string()),
        "{} key",
        path.display()
    );
}

fn assert_root_chain_verifies(
    overlays: &[ArbOverlay],
    vendor_boot: &Path,
    verification_dir: &Path,
) {
    fs::create_dir_all(verification_dir).expect("verification directory");
    for name in ["boot", "vbmeta_system", "vbmeta", "vendor_boot"] {
        let source = if name == "vendor_boot" {
            vendor_boot.to_path_buf()
        } else {
            let overlay_name = if name == "vbmeta" {
                "vbmeta.arb.img"
            } else {
                match name {
                    "boot" => "boot.arb.img",
                    "vbmeta_system" => "vbmeta_system.arb.img",
                    _ => unreachable!(),
                }
            };
            overlay(overlays, overlay_name).to_path_buf()
        };
        fs::copy(&source, verification_dir.join(format!("{name}.img")))
            .unwrap_or_else(|error| panic!("copy {name} for AVB verify: {error}"));
    }

    let expected_key = avbtool_rs::crypto::extract_public_key(OUTPUT_KEY).expect("output key");
    let report = avbtool_rs::verify::verify_image(
        &verification_dir.join("vbmeta.img"),
        &avbtool_rs::verify::VerifyImageOptions {
            key_blob: None,
            expected_chain_partitions: vec![
                avbtool_rs::verify::ExpectedChainPartition {
                    partition_name: "boot".to_string(),
                    rollback_index_location: 0,
                    public_key: expected_key.clone(),
                },
                avbtool_rs::verify::ExpectedChainPartition {
                    partition_name: "vbmeta_system".to_string(),
                    rollback_index_location: 1,
                    public_key: expected_key,
                },
            ],
            follow_chain_partitions: true,
            accept_zeroed_hashtree: false,
        },
    )
    .expect("root and child AVB chain verification");
    assert_eq!(report.verified_images.len(), 3, "root + two chained images");
}

#[test]
fn unchanged_indices_skip_without_force_and_preserve_sources() {
    let fixture = Fixture::new();
    let work = fixture.fresh_work("skip");
    let (overlays, need) = call_builder(&fixture, &work, (5, 15), None, false, None);

    assert!(overlays.is_empty());
    assert!(!need);
    assert!(!work.join("boot.arb.img").exists());
    fixture.assert_originals_unchanged();

    let manual_work = fixture.fresh_work("manual-skip");
    let (manual_overlays, manual_need) = call_builder(
        &fixture,
        &manual_work,
        (5, 15),
        Some(ltbox_patch::rollback::RollbackIndices {
            boot: BOOT_INDEX,
            vbmeta_system: VBMETA_SYSTEM_INDEX,
        }),
        false,
        None,
    );
    assert!(manual_overlays.is_empty());
    assert!(!manual_need);
    assert!(
        fs::read_dir(&manual_work)
            .expect("manual skip work directory")
            .next()
            .is_none()
    );
    fixture.assert_originals_unchanged();
}

#[test]
fn force_resign_uses_rsa4096_exact_indices_and_verifies_chain() {
    let fixture = Fixture::new();
    let work = fixture.fresh_work("force");
    let (overlays, need) = call_builder(&fixture, &work, (5, 15), None, true, None);

    assert!(!need, "force-only resign is not an index downgrade");
    assert_eq!(overlays.len(), 3, "boot, vbmeta_system, and root vbmeta");
    assert_testkey_image(overlay(&overlays, "boot.arb.img"), BOOT_INDEX, 0);
    assert_testkey_image(
        overlay(&overlays, "vbmeta_system.arb.img"),
        VBMETA_SYSTEM_INDEX,
        1,
    );
    let root = overlay(&overlays, "vbmeta.arb.img");
    let root_info = avbtool_rs::image::inspect_avb_image(root).expect("root AVB metadata");
    assert_eq!(root_info.algorithm_name, OUTPUT_ALGORITHM);
    assert_eq!(root_info.public_key_sha1, Some(OUTPUT_KEY_SHA1.to_string()));
    assert_root_chain_verifies(
        &overlays,
        &fixture.firmware.join("vendor_boot.img"),
        &work.join("verify"),
    );
    fixture.assert_originals_unchanged();
}

#[test]
fn automatic_and_manual_targets_are_exact() {
    let fixture = Fixture::new();

    let automatic_work = fixture.fresh_work("automatic");
    let (automatic, automatic_need) =
        call_builder(&fixture, &automatic_work, (15, 25), None, false, None);
    assert!(automatic_need);
    assert_testkey_image(overlay(&automatic, "boot.arb.img"), 15, 0);
    assert_testkey_image(overlay(&automatic, "vbmeta_system.arb.img"), 25, 1);

    let one_child_work = fixture.fresh_work("manual-one-child");
    let (one_child, one_child_need) = call_builder(
        &fixture,
        &one_child_work,
        (5, 15),
        Some(ltbox_patch::rollback::RollbackIndices {
            boot: BOOT_INDEX,
            vbmeta_system: 18,
        }),
        false,
        None,
    );
    assert!(one_child_need);
    assert_testkey_image(overlay(&one_child, "boot.arb.img"), BOOT_INDEX, 0);
    assert_testkey_image(overlay(&one_child, "vbmeta_system.arb.img"), 18, 1);

    let below_firmware_work = fixture.fresh_work("manual-below-firmware");
    let (below_firmware, below_firmware_need) = call_builder(
        &fixture,
        &below_firmware_work,
        (5, 15),
        Some(ltbox_patch::rollback::RollbackIndices {
            boot: 7,
            vbmeta_system: VBMETA_SYSTEM_INDEX,
        }),
        false,
        None,
    );
    assert!(below_firmware_need);
    assert_testkey_image(overlay(&below_firmware, "boot.arb.img"), 7, 0);
    assert_testkey_image(
        overlay(&below_firmware, "vbmeta_system.arb.img"),
        VBMETA_SYSTEM_INDEX,
        1,
    );

    let dual_work = fixture.fresh_work("manual-dual");
    let (dual, dual_need) = call_builder(
        &fixture,
        &dual_work,
        (5, 15),
        Some(ltbox_patch::rollback::RollbackIndices {
            boot: 7,
            vbmeta_system: 18,
        }),
        false,
        None,
    );
    assert!(dual_need);
    assert_testkey_image(overlay(&dual, "boot.arb.img"), 7, 0);
    assert_testkey_image(overlay(&dual, "vbmeta_system.arb.img"), 18, 1);

    fixture.assert_originals_unchanged();
}

#[test]
fn below_floor_fails_before_staging_any_output() {
    let fixture = Fixture::new();
    let work = fixture.fresh_work("invalid-floor");
    let mut log = Vec::new();
    let error = build_testkey_arb_overlays_for_floors(
        &fixture.firmware,
        &work,
        (5, 15),
        Some(ltbox_patch::rollback::RollbackIndices {
            boot: 4,
            vbmeta_system: 15,
        }),
        false,
        None,
        &mut log,
    )
    .expect_err("below-floor manual target must be rejected");
    assert!(
        error.contains("boot") || error.contains("err_flash_manual_rollback_below_floor"),
        "{error}"
    );
    assert!(
        fs::read_dir(&work)
            .expect("work directory")
            .next()
            .is_none()
    );
    fixture.assert_originals_unchanged();
}

#[test]
fn region_vbmeta_base_preserves_unrelated_hash_descriptor() {
    let fixture = Fixture::new();
    let work = fixture.fresh_work("region-base");
    let (overlays, need) = call_builder(
        &fixture,
        &work,
        (5, 15),
        Some(ltbox_patch::rollback::RollbackIndices {
            boot: BOOT_INDEX,
            vbmeta_system: 21,
        }),
        false,
        Some(&fixture.region_vbmeta),
    );
    assert!(need);

    let output_hash =
        ltbox_patch::avb::hash_descriptor(overlay(&overlays, "vbmeta.arb.img"), "vendor_boot")
            .expect("rebuilt vendor_boot hash descriptor");
    let base_hash = ltbox_patch::avb::hash_descriptor(&fixture.region_vbmeta, "vendor_boot")
        .expect("region base vendor_boot hash descriptor");
    assert_eq!(output_hash, base_hash);
    assert_testkey_image(overlay(&overlays, "boot.arb.img"), BOOT_INDEX, 0);
    assert_testkey_image(overlay(&overlays, "vbmeta_system.arb.img"), 21, 1);
    fixture.assert_originals_unchanged();
}

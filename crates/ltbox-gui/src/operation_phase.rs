use std::sync::Arc;

use crate::AdvAction;
use ltbox_core::tr_args;
use ltbox_patch::konabess::KonaBessBuildStage;

pub(crate) fn phase_marker(phase: usize, total: usize, label: impl AsRef<str>) -> String {
    tr_args!(
        "live_phase_marker",
        phase = phase.to_string(),
        total = total.to_string(),
        label = label.as_ref()
    )
}

#[derive(Debug, Clone)]
pub(crate) struct OpStep {
    pub(crate) label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationPhaseKind {
    Flash,
    Root,
    Unroot,
    SysUpdateDisable,
    SysUpdateEnable,
    BootRecovery,
    ChangeCountry,
    DetectArb,
    SimpleFlash,
    FlashPartitions,
    DumpPartitions,
    FlashPhysical,
    DumpPhysical,
    OfflineConvertXml,
    RegionConversion,
    PatchArb,
    RebuildVbmeta,
    KonaBess,
}

impl OperationPhaseKind {
    pub(crate) const fn all() -> &'static [Self] {
        &[
            Self::Flash,
            Self::Root,
            Self::Unroot,
            Self::SysUpdateDisable,
            Self::SysUpdateEnable,
            Self::BootRecovery,
            Self::ChangeCountry,
            Self::DetectArb,
            Self::SimpleFlash,
            Self::FlashPartitions,
            Self::DumpPartitions,
            Self::FlashPhysical,
            Self::DumpPhysical,
            Self::OfflineConvertXml,
            Self::RegionConversion,
            Self::PatchArb,
            Self::RebuildVbmeta,
            Self::KonaBess,
        ]
    }

    pub(crate) const fn for_advanced_file(action: AdvAction) -> Option<Self> {
        match action {
            AdvAction::ConvertXml => Some(Self::OfflineConvertXml),
            AdvAction::RegionConvert => Some(Self::RegionConversion),
            AdvAction::PatchArb => Some(Self::PatchArb),
            AdvAction::RebuildVbmeta => Some(Self::RebuildVbmeta),
            _ => None,
        }
    }

    /// One-based firmware-write phase that may surface live flash progress.
    /// Only full Flash (step 7) and Advanced SimpleFlash (step 3) qualify.
    pub(crate) const fn firmware_progress_step(self) -> Option<usize> {
        match self {
            Self::Flash => Some(7),
            Self::SimpleFlash => Some(3),
            _ => None,
        }
    }

    pub(crate) const fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Flash => &[
                "op_flash_phase_1",
                "op_flash_phase_2",
                "op_flash_phase_3",
                "op_flash_phase_4",
                "op_flash_phase_5",
                "op_flash_phase_6",
                "op_flash_phase_7",
                "op_flash_phase_8",
                "op_flash_phase_9",
            ],
            Self::Root => &[
                "op_root_phase_1",
                "op_root_phase_2",
                "op_root_phase_3",
                "op_root_phase_4",
                "op_root_phase_5",
                "op_root_phase_6",
                "op_root_phase_7",
                "op_root_phase_8",
            ],
            Self::Unroot => &[
                "op_unroot_phase_1",
                "op_unroot_phase_2",
                "op_unroot_phase_3",
                "op_unroot_phase_4",
                "op_unroot_phase_5",
                "op_unroot_phase_6",
            ],
            Self::SysUpdateDisable => &[
                "op_sys_phase_adb",
                "op_sys_disable_phase_policy",
                "op_sys_disable_phase_packages",
            ],
            Self::SysUpdateEnable => &[
                "op_sys_phase_adb",
                "op_sys_enable_phase_policy",
                "op_sys_enable_phase_packages",
            ],
            Self::BootRecovery => &[
                "op_rescue_phase_1",
                "op_rescue_phase_2",
                "op_rescue_phase_3",
                "op_rescue_phase_4",
                "op_rescue_phase_5",
                "op_rescue_phase_6",
                "op_rescue_phase_7",
            ],
            Self::ChangeCountry => &[
                "op_country_phase_validate",
                "op_phase_enter_edl_firehose",
                "op_country_phase_backup",
                "op_country_phase_apply",
                "op_phase_reboot_system",
            ],
            Self::DetectArb => &[
                "op_arb_phase_fastboot",
                "op_arb_phase_read",
                "op_arb_phase_edl",
                "op_arb_phase_result",
                "op_phase_reboot_system",
            ],
            Self::SimpleFlash => &[
                "op_simple_phase_prepare",
                "op_phase_enter_edl_firehose",
                "op_simple_phase_write",
                "op_simple_phase_slot",
                "op_phase_reboot_system",
            ],
            Self::FlashPartitions => &[
                "op_phase_open_firehose",
                "op_flashparts_phase_write",
                "op_phase_reboot_system",
            ],
            Self::DumpPartitions => &[
                "op_phase_open_firehose",
                "op_dumpparts_phase_read",
                "op_phase_stabilize_usb",
                "op_phase_reboot_system",
            ],
            Self::FlashPhysical => &[
                "op_phase_enter_edl",
                "op_phase_open_firehose",
                "op_flashphys_phase_write",
                "op_phase_reboot_system",
            ],
            Self::DumpPhysical => &[
                "op_phase_enter_edl",
                "op_phase_open_firehose",
                "op_dumpphys_phase_read",
                "op_phase_stabilize_usb",
                "op_phase_reboot_system",
            ],
            Self::OfflineConvertXml => &[
                "op_xml_phase_scan",
                "op_xml_phase_decrypt",
                "op_offline_phase_finalize",
            ],
            Self::RegionConversion => &[
                "op_region_phase_validate",
                "op_region_phase_inspect",
                "op_region_phase_patch",
                "op_region_phase_finalize",
            ],
            Self::PatchArb => &[
                "op_patch_arb_phase_inspect",
                "op_patch_arb_phase_keys",
                "op_patch_arb_phase_boot",
                "op_patch_arb_phase_vbmeta",
            ],
            Self::RebuildVbmeta => &[
                "op_vbmeta_phase_inspect",
                "op_vbmeta_phase_rebuild",
                "op_offline_phase_finalize",
            ],
            Self::KonaBess => &[
                "op_konabess_phase_prepare",
                "op_konabess_phase_dump",
                "op_konabess_phase_inspect",
                "op_konabess_phase_patch",
                "op_konabess_phase_rebuild",
                "op_konabess_phase_flash",
                "op_phase_reboot_system",
            ],
        }
    }
}

/// Future Stage-D callbacks map into the stable inspect/patch/rebuild portion
/// of the full EDL operation plan, matching the region worker's stage mapping.
#[allow(dead_code)]
pub(crate) const fn konabess_build_phase(stage: KonaBessBuildStage) -> usize {
    match stage {
        KonaBessBuildStage::Inspect => 3,
        KonaBessBuildStage::PatchVendorBoot => 4,
        KonaBessBuildStage::RebuildVbmeta => 5,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PhaseReporter {
    labels: Arc<[String]>,
}

impl PhaseReporter {
    pub(crate) fn from_labels(labels: Vec<String>) -> Self {
        assert!(
            !labels.is_empty(),
            "operation phase plans must not be empty"
        );
        Self {
            labels: labels.into(),
        }
    }

    pub(crate) fn steps(&self) -> Vec<OpStep> {
        self.labels
            .iter()
            .cloned()
            .map(|label| OpStep { label })
            .collect()
    }

    pub(crate) fn marker(&self, one_based: usize) -> String {
        let index = one_based
            .checked_sub(1)
            .expect("phase markers are one-based");
        let label = self
            .labels
            .get(index)
            .expect("worker marker must exist in its phase plan");
        phase_marker(one_based, self.labels.len(), label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn konabess_build_stages_map_to_inspect_patch_rebuild_phases() {
        assert_eq!(konabess_build_phase(KonaBessBuildStage::Inspect), 3);
        assert_eq!(konabess_build_phase(KonaBessBuildStage::PatchVendorBoot), 4);
        assert_eq!(konabess_build_phase(KonaBessBuildStage::RebuildVbmeta), 5);
    }
}

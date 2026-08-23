//! GUI message types: the top-level [`Message`] plus the per-area
//! sub-message enums it wraps, dispatched by `App::update`.

use crate::{
    AdvAction, ConfirmField, DataMode, DevicePollResult, DeviceRegion, DumpPartsScanResult, Family,
    FlashPartsScanResult, FlashTarget, GpuCellKey, Language, ManualRollbackEditor, NightlySource,
    PartsSortColumn, PickerTarget, Provider, RebootTarget, RescueRegion, RollbackSetting, RootMode,
    SkrootFlavor, SysUpdateAction, ThemeChoice, ThemeSeed, UnrootType, VerChoice, View,
};

#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// No-op for click-blocker mouse_area widgets.
    Noop,
    Navigate(View),
    ResumeBusyOperation,
    /// Open an external URL (About panel links) in the host's default
    /// browser via `open::that_detached` — no in-app webview.
    OpenUrl(&'static str),
    /// Open a runtime-owned URL (e.g. a QFIL download link or the Software Fix
    /// page) in the host's default browser.
    OpenExternalUrl(String),
    SetTheme(ThemeChoice),
    ToggleLogPopup(bool),
    SelectCountry(String),
    SkipCountryPatch,
    DismissCountryPopup,
    SelectRegionTarget(DeviceRegion),
    DismissRegionTargetPopup,
    FileSelected(Option<String>),
    FolderSelected(Option<String>),
    RecentFilePicked(PickerTarget, String),
    RecentFolderPicked(PickerTarget, String),
    NoticeRecentMissing(bool),
    OperationError(String),
    DismissError,
    StartOver,
    PollDevice,
    DevicePolled(DevicePollResult),
    /// Dashboard "Kill Server" button fired when an external adb
    /// server is holding the Android USB interface — sends `host:kill`
    /// to `127.0.0.1:5037` so LTBox's libusb claim can succeed on the
    /// next poll.
    KillAdbServer,
    /// Click on the dashboard device portrait. Opens the popup; fires
    /// the Lenovo PTSTPD fetch unless the serial is already cached.
    DeviceInfoOpen,
    /// Result of the PTSTPD fetch keyed by the serial it was started for.
    /// Stale results (different serial than the currently open popup)
    /// are still cached for next time but do not flip the popup state.
    DeviceInfoFetched(String, Result<ltbox_core::lenovo_info::MachineInfo, String>),
    /// User dismissed the device-info popup.
    DeviceInfoClose,
    /// Retry fetch for the currently open popup serial.
    DeviceInfoRetry,
    /// Click on the dashboard firmware version. Opens the OTA popup
    /// and fires the upstream `querynewfirmware` request.
    OtaOpen,
    /// Result of the OTA fetch, keyed by the (serial, firmware-id)
    /// pair the request was started for so a stale device swap can't
    /// surface the wrong firmware's changelog.
    OtaFetched(
        String,
        String,
        Result<Option<ltbox_core::lenovo_ota::OtaUpdate>, String>,
    ),
    /// User dismissed the OTA popup.
    OtaClose,
    /// Retry fetch for the currently open OTA popup query.
    OtaRetry,
    /// Open the OTA download URL in the host's default browser.
    OtaOpenDownload(String),
    /// Read-only forward of `text_editor::Action` for the OTA popup's
    /// changelog editor. Edit actions are dropped so the user can
    /// drag-select / Ctrl+C without mutating the changelog buffer.
    OtaChangelogAction(iced::widget::text_editor::Action),
    /// Open/close the firmware-version dropdown (QFIL Firmware / OTA Package).
    FirmwareMenu(bool),
    /// Click the "QFIL Firmware" menu item. Resolves the device MTM, then the
    /// official QFIL package (CN-only), and opens the QFIL popup.
    QfilOpen,
    /// Result of the QFIL fetch, keyed by the serial it was started for.
    QfilFetched(String, Result<crate::QfilOutcome, String>),
    /// User dismissed the QFIL popup.
    QfilClose,
    /// Retry the QFIL fetch for the currently open popup serial.
    QfilRetry,
    /// Copy `payload` to the OS clipboard. Pairs with `ToastShow` so
    /// the user gets a visual confirmation; clipboard writes return a
    /// `Task<Message>` from iced so the second message is chained.
    CopyToClipboard(String),
    /// Show a transient bottom-of-screen toast message. Auto-clears
    /// via `ToastClear` after a short delay.
    ToastShow(String),
    /// Clear the active toast (timer expiry).
    ToastClear,
    /// Sidebar mouse-area entered — expand to full width.
    SidebarHoverEnter,
    /// Sidebar mouse-area exited — collapse back to icon-only width.
    SidebarHoverExit,
    /// Periodic system-theme probe while "Follow system" is active.
    RefreshSystemTheme,
    /// 16 ms tick from the sidebar tween subscription. Steps
    /// `sidebar_anim` toward its target via exponential decay.
    /// Subscription auto-stops once the value has settled.
    SidebarAnimTick,
    /// Dashboard rollback cell → open the `boot` / `vbmeta_system`
    /// floor breakdown. Only reachable in bootloader mode.
    RollbackDetailOpen,
    RollbackDetailClose,
    /// Step the popup's value rendering raw → unix → date → raw.
    RollbackDetailCycleFormat,
    DriverCheckDone(ltbox_device::driver::DriverStatus),
    InstallDrivers,
    InstallDriversDone(Result<Vec<String>, String>),
    UpdateCheckDone(Option<ltbox_core::github::StableRelease>),
    OpenUpdateUrl,
    /// Startup GitHub-reachability probe result. Gates the driver
    /// install/update buttons (offline → disabled + "needs internet" tip).
    ConnectivityChecked(bool),
    /// Startup Qualcomm-driver version check result. `Some` → installed
    /// driver is older than the latest release; drives the optional update
    /// banner. `None` → up to date / not installed / offline (no banner).
    DriverUpdateCheckDone(Option<ltbox_device::driver::DriverUpdate>),
    /// "Don't show again" on the driver-update banner — persist the
    /// dismissal and drop the banner for the rest of the session.
    DismissDriverUpdate,
    /// "Don't show again" on the dual-USB-C port advisory for the given
    /// model — persist it so that model never shows the advisory again.
    DismissDualUsbAdvisory(String),
    /// "Close" on the dual-USB-C port advisory for the given model — hide it
    /// for this session only (returns on the next launch).
    CloseDualUsbAdvisory(String),
    DrainStdoutTap,
    LogEditorAction(iced::widget::text_editor::Action),
    ImageInfoLogEditorAction(iced::widget::text_editor::Action),
    SaveLog,
    SaveLogPath(Option<std::path::PathBuf>),
    Window(WindowMsg),
    Flash(FlashMsg),
    Root(RootMsg),
    Unroot(UnrootMsg),
    Sys(SysMsg),
    Adv(AdvMsg),
    KonaBess(KonaBessMsg),
    FlashParts(FlashPartsMsg),
    DumpParts(DumpPartsMsg),
    DumpPhys(DumpPhysMsg),
    FlashPhys(FlashPhysMsg),
    SimpleFlash(SimpleFlashMsg),
    Reboot(RebootMsg),
    Settings(SettingsMsg),
    /// Window resized — carries the new logical size from
    /// `iced::Event::Window(Resized)`. Persisted with throttling so the
    /// user's preferred geometry survives a restart.
    WindowResized(f32, f32),
    /// Current host maximized state. Queried from iced because window
    /// events expose resize/move but not a dedicated maximize notification.
    WindowMaximized(bool),
    /// Tick from a periodic subscription; flushes the latest window
    /// size to disk if `window_size_dirty` is set and the debounce
    /// interval has elapsed since the last save.
    PersistWindowSize,
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum WindowMsg {
    WindowIdReceived(Option<iced::window::Id>),
    WindowDrag,
    WindowMinimize,
    WindowToggleMaximize,
    WindowClose,
    /// Cursor-drag resize emitted by the invisible edge/corner
    /// handles overlaid on the root Stack. The borderless titlebar
    /// removes native winit resize edges, so the GUI synthesizes them.
    WindowResize(iced::window::Direction),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum FlashMsg {
    FlashRegion(DeviceRegion),
    /// Result of the on-entry auto-region PTSTPD fetch: `(probe_id, serial,
    /// result)`. The monotonic `probe_id` is the staleness token — only the
    /// currently-pending probe is applied. Preselects PRC/ROW from SaleArea
    /// and advances past the step.
    FlashAutoRegionFetched(
        u64,
        String,
        Result<ltbox_core::lenovo_info::MachineInfo, String>,
    ),
    /// Manual-serial prompt: typing.
    FlashSerialPromptInput(String),
    /// Manual-serial prompt: submit the entered serial and query.
    FlashSerialPromptSubmit,
    /// Manual-serial prompt: skip auto-detect and pick the region manually.
    FlashSerialPromptSkip,
    FlashTarget(FlashTarget),
    FlashDataMode(DataMode),
    FlashNext,
    FlashBack,
    FlashSelectFolder,
    /// Pick a standalone EDL loader when the firmware folder ships none.
    FlashSelectLoader,
    FlashLoaderChosen(Option<String>),
    /// Confirm-step "hidden dropdown": open the option editor for a row.
    FlashConfirmOpen(ConfirmField),
    /// Dismiss the confirm-step option editor without a change.
    FlashConfirmClose,
    FlashConfirmSetRegion(DeviceRegion),
    FlashConfirmSetTarget(FlashTarget),
    FlashConfirmSetData(DataMode),
    FlashConfirmSetRegionEdit(bool),
    FlashConfirmSetRollback(RollbackSetting),
    FlashManualRollbackInput(ManualRollbackEditor, String),
    FlashManualRollbackCycleFormat,
    FlashManualRollbackCancel,
    FlashManualRollbackConfirm,
    FlashExecStart,
    FlashExecDone(Vec<String>),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum RootMsg {
    RootFamily(Family),
    RootProvider(Provider),
    RootMode(RootMode),
    RootSkrootFlavor(SkrootFlavor),
    RootVersion(VerChoice),
    RootNightlySource(NightlySource),
    RootSelectFile,
    RootSelectFolder,
    RootNext,
    RootBack,
    RootSelectKpm,
    RootKpmSelected(Option<Vec<String>>),
    RootKpmRemove(String),
    RootSuperkeyInput(String),
    RootSuperkeyConfirm,
    RootSuperkeyCancel,
    RootRunIdInput(String),
    RootRunIdConfirm,
    RootRunIdCancel,
    RootKernelVersionInput(String),
    RootKernelVersionConfirm,
    RootKernelVersionCancel,
    /// Result of the off-UI-thread ADB probe started by `RootNext` when
    /// the wizard hits step 6 with `needs_ksu_lkm_kernel_version()`.
    /// `Some(kver)` advances the wizard; `None` opens the manual-input
    /// popup.
    RootKernelVersionProbeDone(Option<String>),
    RootExecStart,
    RootExecDone(Vec<String>),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum UnrootMsg {
    SetUnrootType(UnrootType),
    UnrootSelectFolder,
    UnrootSelectLoader,
    UnrootLoaderChosen(Option<String>),
    UnrootNext,
    UnrootBack,
    UnrootExecStart,
    UnrootExecDone(Vec<String>),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum SysMsg {
    SysAction(SysUpdateAction),
    SysNext,
    SysBack,
    SysExecStart,
    SysExecDone(Vec<String>),
    SysRescueSelectFolder,
    SysRescueFolderChosen(Option<String>),
    SysRescueRegion(RescueRegion),
    SysRescueRegionPopupDismiss,
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum AdvMsg {
    AdvConfirm(AdvAction),
    AdvExec(AdvAction),
    AdvExecDone(Vec<String>),
    AdvFileSelected(AdvAction, Option<String>),
    AdvWizOpen(AdvAction),
    AdvWizBack,
    AdvWizNext,
    AdvWizBrowse,
    AdvWizBrowseDone(Option<String>),
    AdvWizBrowseManyDone(Option<Vec<String>>),
    AdvImageInfoExecStart,
    AdvImageInfoExecDone(Result<String, String>),
    /// DetectArb: kicks off the fastboot+EDL anti-rollback probe on
    /// the heavy pool. Triggered by Next on the source step.
    AdvDetectArbExecStart,
    /// DetectArb worker result. `Vec<String>` is the live-log lines
    /// to flush; `Err(_)` carries a banner message.
    AdvDetectArbExecDone(Result<Vec<String>, String>),
    AdvWizOpenCountry,
    AdvWizOpenRegionTarget,
    AdvWizOpenOutputFolder,
    /// PatchArb timestamp popup: live-typing input.
    AdvWizArbIndexInput(String),
    /// PatchArb timestamp popup: OK pressed (only valid when the buffer
    /// is exactly 10 digits — UI gates this).
    AdvWizArbIndexConfirm,
    /// PatchArb timestamp popup: cancel — closes the popup, clears the
    /// buffer, leaves the wizard on the source step.
    AdvWizArbIndexCancel,
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum KonaBessMsg {
    KonaBessSelectLoader,
    KonaBessLoaderChosen(Option<String>),
    KonaBessSelectImport,
    KonaBessImportChosen(Option<String>),
    KonaBessOpenTarget,
    KonaBessRevertEdits,
    KonaBessCellChanged(GpuCellKey, String),
    KonaBessAddLevel(usize),
    KonaBessRemoveLevel(usize, usize),
    KonaBessNext,
    KonaBessBack,
    /// Feed the retained workspace plus parsed device GPU tables into the
    /// target-selection transition.
    KonaBessInspectionReady(crate::KonaBessInspectionResult),
    KonaBessInspectionFailed(String),
    KonaBessTargetSelected(usize),
    KonaBessTargetConfirm,
    KonaBessTargetDismiss,
    KonaBessCancelDone(Vec<String>),
    KonaBessFlashDone(Vec<String>),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum FlashPartsMsg {
    FlashPartsSelectLoader,
    FlashPartsLoaderChosen(Option<String>),
    FlashPartsToggleRow(usize),
    FlashPartsPickRowFile(usize),
    FlashPartsRowFileChosen(usize, Option<String>),
    FlashPartsNext,
    FlashPartsBack,
    FlashPartsClose,
    FlashPartsScanStart,
    FlashPartsScanDone(FlashPartsScanResult),
    FlashPartsExecStart,
    FlashPartsExecDone(Vec<String>),
    /// Header click in the Select-step table.
    FlashPartsSortBy(PartsSortColumn),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum DumpPartsMsg {
    DumpPartsSelectLoader,
    DumpPartsLoaderChosen(Option<String>),
    DumpPartsToggleRow(usize),
    DumpPartsNext,
    DumpPartsBack,
    DumpPartsClose,
    DumpPartsScanStart,
    DumpPartsScanDone(DumpPartsScanResult),
    DumpPartsSelectFolder,
    DumpPartsFolderChosen(Option<String>),
    DumpPartsExecDone(Vec<String>),
    /// Header click in the Select-step table.
    DumpPartsSortBy(PartsSortColumn),
    /// Header checkbox: select-all when any unselected, otherwise clear.
    DumpPartsToggleAll,
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum DumpPhysMsg {
    DumpPhysSelectLoader,
    DumpPhysLoaderChosen(Option<String>),
    DumpPhysToggleRow(usize),
    DumpPhysNext,
    DumpPhysBack,
    DumpPhysClose,
    DumpPhysSelectFolder,
    DumpPhysFolderChosen(Option<String>),
    DumpPhysExecDone(Vec<String>),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum FlashPhysMsg {
    FlashPhysSelectLoader,
    FlashPhysLoaderChosen(Option<String>),
    FlashPhysToggleRow(usize),
    FlashPhysPickRowFile(usize),
    FlashPhysRowFileChosen(usize, Option<String>),
    FlashPhysNext,
    FlashPhysBack,
    FlashPhysClose,
    FlashPhysExecStart,
    FlashPhysExecDone(Vec<String>),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum SimpleFlashMsg {
    SimpleFlashNext,
    SimpleFlashBack,
    SimpleFlashClose,
    SimpleFlashSelectFolder,
    SimpleFlashFolderChosen(Option<String>),
    SimpleFlashExecStart,
    SimpleFlashExecDone(Vec<String>),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum RebootMsg {
    RebootRequest(RebootTarget),
    RebootConfirm,
    RebootDismiss,
    RebootTo(RebootTarget),
    RebootEdlWithLoader(RebootTarget, Option<String>),
    RebootDone(Vec<String>),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum SettingsMsg {
    SetLanguage(Language),
    SetThemeSeed(ThemeSeed),
    SetQcomDriverMode(ltbox_device::driver::QcomDriverMode),
    SettingsPickDefaultLoader,
    SettingsDefaultLoaderChosen(Option<String>),
    SettingsClearDefaultLoader,
    /// Remove leftover temp files (`work_*` scratch + `output_*` auto-output).
    CleanupTempFiles,
    /// Cleanup sweep finished; triggers a rescan.
    CleanupDone,
    /// Result of a temp-file size scan, in bytes. Drives the button's
    /// enabled state + the size readout next to the label.
    TempScanDone(u64),
}

//! Action map — every possible user action in Phosphor.
//!
//! This is the single source of truth for what the user can do.
//! The key handler maps keys → actions. Tests map scenarios → action sequences.
//! No key codes in business logic, no business logic in key handling.

/// Every discrete action a user can perform in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // ── Global ──
    Quit,
    OpenSpaceMenu,
    CloseSpaceMenu,
    NextPane,
    PrevPane,

    // ── Space menu ──
    SpaceMenuUp,
    SpaceMenuDown,
    SpaceMenuSelect,
    SpaceMenuSwitchTab,
    SpaceMenuKey(char),

    // ── Transport (via space menu) ──
    PlayPause,
    ToggleRecord,
    Panic,
    Save,

    // ── Loop editor ──
    FocusLoopEditor,
    LoopToggleEnabled,
    LoopStartLeft,
    LoopStartRight,
    LoopEndLeft,
    LoopEndRight,
    LoopUnfocus,

    // ── Track navigation ──
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Select,
    Back,

    // ── Track controls ──
    ToggleMute,
    ToggleSolo,
    ToggleArm,
    ToggleLoopRecord,

    // ── Instrument ──
    AddInstrument,
    InstrumentSelect,
    InstrumentCancel,

    // ── Clip view ──
    CycleTab,

    // ── Synth params (when in clip view synth panel) ──
    ParamUp,
    ParamDown,
    ParamDecrease,
    ParamIncrease,

    // ── No-op ──
    None,
}

//! Native macOS modifier-key state used when a terminal drops PTY modifiers.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ModifierState {
    pub(crate) command: bool,
    pub(crate) option: bool,
    pub(crate) shift: bool,
}

impl ModifierState {
    pub(crate) fn any_newline_modifier(self) -> bool {
        self.command || self.option || self.shift
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn snapshot() -> ModifierState {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }

    const HID_SYSTEM_STATE: i32 = 1;
    const SHIFT_MASK: u64 = 0x0002_0000;
    const OPTION_MASK: u64 = 0x0008_0000;
    const COMMAND_MASK: u64 = 0x0010_0000;

    // SAFETY: this stable CoreGraphics function takes and returns integers and
    // does not transfer pointers or ownership across the FFI boundary.
    let flags = unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) };
    ModifierState {
        command: flags & COMMAND_MASK != 0,
        option: flags & OPTION_MASK != 0,
        shift: flags & SHIFT_MASK != 0,
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn snapshot() -> ModifierState {
    ModifierState::default()
}

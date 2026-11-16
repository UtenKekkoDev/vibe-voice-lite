//! Hold-to-talk trigger key.
//!
//! The Windows global-hotkey API only reports key presses, so a hold gesture is
//! detected by polling the physical key state instead of hooking the keyboard.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

/// Trigger keys offered in settings. Right-hand modifiers are preferred because
/// they are rarely bound to editor or IME commands.
pub const OPTIONS: [&str; 5] = ["RightCtrl", "RightAlt", "RightShift", "F9", "F10"];

pub fn virtual_key(name: &str) -> i32 {
    match name {
        "RightAlt" => 0xA5,
        "RightShift" => 0xA1,
        "F9" => 0x78,
        "F10" => 0x79,
        _ => 0xA3, // RightCtrl
    }
}

pub fn label(name: &str) -> &'static str {
    match name {
        "RightAlt" => "右 Alt",
        "RightShift" => "右 Shift",
        "F9" => "F9",
        "F10" => "F10",
        _ => "右 Ctrl",
    }
}

pub fn is_down(name: &str) -> bool {
    // GetAsyncKeyState reads the async key table; no message pump is required.
    unsafe { (GetAsyncKeyState(virtual_key(name)) as u16 & 0x8000) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_map_to_distinct_keys_and_fall_back_safely() {
        let mut seen = std::collections::HashSet::new();
        for name in OPTIONS {
            assert!(seen.insert(virtual_key(name)), "duplicate key for {name}");
            assert!(!label(name).is_empty());
        }
        assert_eq!(virtual_key("RightCtrl"), 0xA3);
        assert_eq!(virtual_key("something-else"), 0xA3);
    }
}

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::thread;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, HC_ACTION,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    Pressed,
    Released,
}

static ACTIVATION_VK: AtomicU32 = AtomicU32::new(0);
static HOTKEY_STATE: OnceLock<Mutex<HotkeyState>> = OnceLock::new();
static HOOK_SENDER: OnceLock<Mutex<Option<Sender<HookEvent>>>> = OnceLock::new();

const VK_MENU: u32 = 0x12;
const VK_LMENU: u32 = 0xA4;
const VK_RMENU: u32 = 0xA5;
const ALT_SCAN_CODE: u32 = 0x38;
const LLKHF_EXTENDED_BIT: u32 = 0x01;

fn debug_log(message: impl AsRef<str>) {
    let path = std::env::temp_dir().join("voca-hotkey-debug.log");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| {
            use std::io::Write;
            writeln!(file, "{}", message.as_ref())
        });
}

#[derive(Debug, Default)]
struct HotkeyState {
    accepted_hotkey_down: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct HookDecision {
    event: Option<HookEvent>,
    swallow: bool,
}

impl HotkeyState {
    fn handle_event(
        &mut self,
        message: u32,
        vk_code: u32,
        scan_code: u32,
        flags: u32,
        activation_vk: u32,
    ) -> HookDecision {
        if !key_matches(vk_code, scan_code, flags, activation_vk) {
            return HookDecision::default();
        }

        match message {
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let event = if self.accepted_hotkey_down {
                    None
                } else {
                    self.accepted_hotkey_down = true;
                    Some(HookEvent::Pressed)
                };
                HookDecision {
                    event,
                    swallow: true,
                }
            }
            WM_KEYUP | WM_SYSKEYUP => {
                let event = if self.accepted_hotkey_down {
                    self.accepted_hotkey_down = false;
                    Some(HookEvent::Released)
                } else {
                    None
                };
                HookDecision {
                    event,
                    swallow: true,
                }
            }
            _ => HookDecision::default(),
        }
    }
}

pub fn start(key_name: &str, sender: Sender<HookEvent>) -> Result<String, String> {
    let vk = activation_vk(key_name).unwrap_or(0x77);
    debug_log(format!("start key={key_name} activation_vk=0x{vk:X}"));
    ACTIVATION_VK.store(vk, Ordering::Relaxed);
    let state_cell = HOTKEY_STATE.get_or_init(|| Mutex::new(HotkeyState::default()));
    *state_cell
        .lock()
        .map_err(|_| "Keyboard hook state lock failed".to_string())? = HotkeyState::default();
    let sender_cell = HOOK_SENDER.get_or_init(|| Mutex::new(None));
    *sender_cell
        .lock()
        .map_err(|_| "Keyboard hook sender lock failed".to_string())? = Some(sender);

    thread::Builder::new()
        .name("voca-keyboard-hook".into())
        .spawn(move || unsafe {
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) {
                Ok(hook) => {
                    debug_log("hook installed");
                    hook
                }
                Err(error) => {
                    debug_log(format!("hook installation failed: {error}"));
                    return;
                }
            };

            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(hook);
        })
        .map_err(|e| format!("Failed to start keyboard hook: {}", e))?;

    Ok(format!("Hold {} to dictate", display_key(vk)))
}

fn activation_vk(key_name: &str) -> Option<u32> {
    match key_name.trim().to_ascii_lowercase().as_str() {
        "alt" | "leftalt" => Some(0x12),
        "rightalt" | "altgr" => Some(0xA5),
        "f8" => Some(0x77),
        "f9" => Some(0x78),
        "ctrl" | "control" => Some(0x11),
        _ => None,
    }
}

fn display_key(vk: u32) -> &'static str {
    match vk {
        0x12 => "Alt",
        0xA5 => "Right Alt",
        0x77 => "F8",
        0x78 => "F9",
        0x11 => "Ctrl",
        _ => "F8",
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let message = wparam.0 as u32;
        let keyboard = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let decision = HOTKEY_STATE
            .get_or_init(|| Mutex::new(HotkeyState::default()))
            .lock()
            .map(|mut state| {
                state.handle_event(
                    message,
                    keyboard.vkCode,
                    keyboard.scanCode,
                    keyboard.flags.0,
                    ACTIVATION_VK.load(Ordering::Relaxed),
                )
            })
            .unwrap_or_default();

        if matches!(keyboard.vkCode, VK_MENU | VK_LMENU | VK_RMENU)
            || keyboard.scanCode == ALT_SCAN_CODE
        {
            debug_log(format!(
                "message=0x{message:X} vk=0x{:X} scan=0x{:X} flags=0x{:X} decision={decision:?}",
                keyboard.vkCode, keyboard.scanCode, keyboard.flags.0
            ));
        }

        if let Some(event) = decision.event {
            send_event(event);
        }
        if decision.swallow {
            return LRESULT(1);
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn key_matches(vk_code: u32, scan_code: u32, flags: u32, activation_vk: u32) -> bool {
    match activation_vk {
        VK_MENU => matches!(vk_code, VK_MENU | VK_LMENU | VK_RMENU),
        VK_RMENU => {
            matches!(vk_code, VK_MENU | VK_RMENU)
                && scan_code == ALT_SCAN_CODE
                && flags & LLKHF_EXTENDED_BIT != 0
        }
        _ => vk_code == activation_vk,
    }
}

fn send_event(event: HookEvent) {
    if let Some(sender_cell) = HOOK_SENDER.get() {
        if let Ok(guard) = sender_cell.lock() {
            if let Some(sender) = guard.as_ref() {
                let _ = sender.send(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn left_alt_down(state: &mut HotkeyState) -> HookDecision {
        state.handle_event(WM_SYSKEYDOWN, VK_LMENU, ALT_SCAN_CODE, 0, VK_RMENU)
    }

    fn left_alt_up(state: &mut HotkeyState) -> HookDecision {
        state.handle_event(WM_SYSKEYUP, VK_LMENU, ALT_SCAN_CODE, 0, VK_RMENU)
    }

    fn right_alt_down(state: &mut HotkeyState) -> HookDecision {
        state.handle_event(
            WM_SYSKEYDOWN,
            VK_RMENU,
            ALT_SCAN_CODE,
            LLKHF_EXTENDED_BIT,
            VK_RMENU,
        )
    }

    fn right_alt_up(state: &mut HotkeyState) -> HookDecision {
        state.handle_event(
            WM_SYSKEYUP,
            VK_RMENU,
            ALT_SCAN_CODE,
            LLKHF_EXTENDED_BIT,
            VK_RMENU,
        )
    }

    #[test]
    fn left_alt_down_emits_nothing_and_passes_through() {
        let mut state = HotkeyState::default();
        assert_eq!(left_alt_down(&mut state), HookDecision::default());
        assert!(!state.accepted_hotkey_down);
    }

    #[test]
    fn left_alt_up_emits_nothing_and_passes_through() {
        let mut state = HotkeyState::default();
        assert_eq!(left_alt_up(&mut state), HookDecision::default());
        assert!(!state.accepted_hotkey_down);
    }

    #[test]
    fn left_alt_tap_emits_nothing() {
        let mut state = HotkeyState::default();
        assert_eq!(left_alt_down(&mut state), HookDecision::default());
        assert_eq!(left_alt_up(&mut state), HookDecision::default());
    }

    #[test]
    fn right_alt_down_emits_one_pressed_event() {
        let mut state = HotkeyState::default();
        assert_eq!(right_alt_down(&mut state).event, Some(HookEvent::Pressed));
        assert!(state.accepted_hotkey_down);
    }

    #[test]
    fn right_alt_up_after_accepted_down_emits_released() {
        let mut state = HotkeyState::default();
        right_alt_down(&mut state);
        assert_eq!(right_alt_up(&mut state).event, Some(HookEvent::Released));
        assert!(!state.accepted_hotkey_down);
    }

    #[test]
    fn right_alt_up_without_accepted_down_emits_nothing() {
        let mut state = HotkeyState::default();
        assert_eq!(right_alt_up(&mut state).event, None);
        assert!(!state.accepted_hotkey_down);
    }

    #[test]
    fn repeated_right_alt_down_emits_pressed_once() {
        let mut state = HotkeyState::default();
        assert_eq!(right_alt_down(&mut state).event, Some(HookEvent::Pressed));
        assert_eq!(right_alt_down(&mut state).event, None);
        assert!(state.accepted_hotkey_down);
    }

    #[test]
    fn left_alt_events_do_not_change_an_accepted_right_alt_press() {
        let mut state = HotkeyState::default();
        right_alt_down(&mut state);
        assert_eq!(left_alt_down(&mut state), HookDecision::default());
        assert_eq!(left_alt_up(&mut state), HookDecision::default());
        assert!(state.accepted_hotkey_down);
        assert_eq!(right_alt_up(&mut state).event, Some(HookEvent::Released));
    }

    #[test]
    fn alt_configuration_accepts_either_alt_key() {
        let mut left_state = HotkeyState::default();
        let mut right_state = HotkeyState::default();
        assert_eq!(
            left_state
                .handle_event(WM_SYSKEYDOWN, VK_LMENU, ALT_SCAN_CODE, 0, VK_MENU)
                .event,
            Some(HookEvent::Pressed)
        );
        assert_eq!(
            right_state
                .handle_event(
                    WM_SYSKEYDOWN,
                    VK_RMENU,
                    ALT_SCAN_CODE,
                    LLKHF_EXTENDED_BIT,
                    VK_MENU,
                )
                .event,
            Some(HookEvent::Pressed)
        );
    }

    #[test]
    fn f8_and_f9_state_transitions_are_unchanged() {
        for activation_vk in [0x77, 0x78] {
            let mut state = HotkeyState::default();
            assert_eq!(
                state
                    .handle_event(WM_KEYDOWN, activation_vk, 0, 0, activation_vk)
                    .event,
                Some(HookEvent::Pressed)
            );
            assert_eq!(
                state
                    .handle_event(WM_KEYUP, activation_vk, 0, 0, activation_vk)
                    .event,
                Some(HookEvent::Released)
            );
        }

        let mut state = HotkeyState::default();
        assert_eq!(
            state.handle_event(WM_KEYDOWN, 0x41, 0x1E, 0, 0x77),
            HookDecision::default()
        );
    }
}

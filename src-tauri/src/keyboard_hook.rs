use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::thread;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    HC_ACTION, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

#[derive(Debug, Clone, Copy)]
pub enum HookEvent {
    Pressed,
    Released,
}

static ACTIVATION_VK: AtomicU32 = AtomicU32::new(0);
static KEY_IS_DOWN: AtomicBool = AtomicBool::new(false);
static HOOK_SENDER: OnceLock<Mutex<Option<Sender<HookEvent>>>> = OnceLock::new();

pub fn start(key_name: &str, sender: Sender<HookEvent>) -> Result<String, String> {
    let vk = activation_vk(key_name).unwrap_or(0x77);
    ACTIVATION_VK.store(vk, Ordering::Relaxed);
    let sender_cell = HOOK_SENDER.get_or_init(|| Mutex::new(None));
    *sender_cell
        .lock()
        .map_err(|_| "Keyboard hook sender lock failed".to_string())? = Some(sender);

    thread::Builder::new()
        .name("voca-keyboard-hook".into())
        .spawn(move || unsafe {
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) {
                Ok(hook) => hook,
                Err(_) => return,
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
        let event = wparam.0 as u32;
        let keyboard = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if key_matches(keyboard.vkCode, ACTIVATION_VK.load(Ordering::Relaxed)) {
            match event {
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    if !KEY_IS_DOWN.swap(true, Ordering::Relaxed) {
                        send_event(HookEvent::Pressed);
                    }
                }
                WM_KEYUP | WM_SYSKEYUP => {
                    if KEY_IS_DOWN.swap(false, Ordering::Relaxed) {
                        send_event(HookEvent::Released);
                    }
                }
                _ => {}
            }
            return LRESULT(1);
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn key_matches(vk_code: u32, activation_vk: u32) -> bool {
    if activation_vk == 0x12 {
        matches!(vk_code, 0x12 | 0xA4 | 0xA5)
    } else {
        vk_code == activation_vk
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

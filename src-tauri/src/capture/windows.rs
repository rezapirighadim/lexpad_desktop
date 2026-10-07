//! Windows: UI Automation's TextPattern first, the clipboard second.
//!
//! Neither needs a permission. UI Automation reads the focused element's
//! selection (and the paragraph around it) without touching anything; where
//! the app does not support it, the clipboard is saved, Ctrl+C is sent with
//! SendInput, the text is read and the clipboard is put back.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HGLOBAL};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, TextUnit_Paragraph, UIA_TextPatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::{tidy, Capture, Permission, Via, CONTEXT_EACH_SIDE, MAX_TEXT};

const CF_UNICODETEXT: u32 = 13;
/// Formats whose handle is not global memory (GDI objects, metafiles, owner
/// display). They cannot be copied as bytes; Windows synthesises the bitmap
/// ones again from CF_DIB, which is kept.
const NOT_MEMORY: [u32; 8] = [2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];
const VK_C: VIRTUAL_KEY = VIRTUAL_KEY(0x43);

pub fn capture() -> Capture {
    let app = foreground_app();
    if let Some(mut c) = from_automation() {
        c.app = app;
        return c;
    }
    match from_clipboard() {
        Some(text) => Capture {
            text: Some(text),
            context: None,
            app,
            permission: Permission::NotNeeded,
            via: Via::Clipboard,
            anchor: None,
        },
        None => Capture::empty(Permission::NotNeeded, app),
    }
}

/// The app in front, by the description in its executable ("Microsoft
/// Word"), or the file name without `.exe` when it has none.
fn foreground_app() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        file_description(&path).or_else(|| {
            std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
    }
}

fn file_description(path: &str) -> Option<String> {
    unsafe {
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let name = PCWSTR(wide.as_ptr());
        let size = GetFileVersionInfoSizeW(name, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(name, None, size, data.as_mut_ptr() as *mut c_void).ok()?;
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(
            data.as_ptr() as *const c_void,
            w!("\\VarFileInfo\\Translation"),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || len < 4
        {
            return None;
        }
        let lang = *(ptr as *const u16);
        let codepage = *(ptr as *const u16).add(1);
        let key: Vec<u16> = format!("\\StringFileInfo\\{lang:04x}{codepage:04x}\\FileDescription")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        if !VerQueryValueW(
            data.as_ptr() as *const c_void,
            PCWSTR(key.as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || len == 0
        {
            return None;
        }
        let text = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        let s = String::from_utf16_lossy(&text[..end]).trim().to_owned();
        (!s.is_empty()).then_some(s)
    }
}

fn from_automation() -> Option<Capture> {
    unsafe {
        // Already initialised on this thread is fine; anything else means no UIA.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let element = automation.GetFocusedElement().ok()?;
        // Never read a password field.
        if element
            .CurrentIsPassword()
            .map(|b| b.as_bool())
            .unwrap_or(true)
        {
            return None;
        }
        let pattern: IUIAutomationTextPattern =
            element.GetCurrentPatternAs(UIA_TextPatternId).ok()?;
        let ranges = pattern.GetSelection().ok()?;
        if ranges.Length().ok()? == 0 {
            return None;
        }
        let range = ranges.GetElement(0).ok()?;
        let text = tidy(&range.GetText(MAX_TEXT as i32).ok()?.to_string())?;
        // The paragraph around the selection, when the app gives one.
        let context = range.Clone().ok().and_then(|whole| {
            whole.ExpandToEnclosingUnit(TextUnit_Paragraph).ok()?;
            let all = whole
                .GetText((CONTEXT_EACH_SIDE * 4) as i32)
                .ok()?
                .to_string();
            (!all.trim().is_empty()).then_some(all)
        });
        Some(Capture {
            text: Some(text),
            context,
            app: None,
            permission: Permission::NotNeeded,
            via: Via::Accessibility,
            anchor: None,
        })
    }
}

fn open_clipboard() -> bool {
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Every format on the clipboard that is plain memory, as bytes.
fn save() -> Option<Vec<(u32, Vec<u8>)>> {
    if !open_clipboard() {
        return None;
    }
    let mut out = Vec::new();
    unsafe {
        let mut format = 0u32;
        loop {
            format = EnumClipboardFormats(format);
            if format == 0 {
                break;
            }
            if NOT_MEMORY.contains(&format) {
                continue;
            }
            let Ok(handle) = GetClipboardData(format) else {
                continue;
            };
            let global = HGLOBAL(handle.0);
            let size = GlobalSize(global);
            let p = GlobalLock(global);
            if !p.is_null() && size > 0 {
                out.push((
                    format,
                    std::slice::from_raw_parts(p as *const u8, size).to_vec(),
                ));
            }
            let _ = GlobalUnlock(global);
        }
        let _ = CloseClipboard();
    }
    Some(out)
}

fn restore(saved: Vec<(u32, Vec<u8>)>) {
    if !open_clipboard() {
        return;
    }
    unsafe {
        let _ = EmptyClipboard();
        for (format, bytes) in saved {
            let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else {
                continue;
            };
            let p = GlobalLock(global);
            if p.is_null() {
                continue;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
            let _ = GlobalUnlock(global);
            // The clipboard owns the memory once this succeeds.
            let _ = SetClipboardData(format, Some(HANDLE(global.0)));
        }
        let _ = CloseClipboard();
    }
}

fn read_text() -> Option<String> {
    if !open_clipboard() {
        return None;
    }
    let text = unsafe {
        GetClipboardData(CF_UNICODETEXT).ok().and_then(|handle| {
            let global = HGLOBAL(handle.0);
            let p = GlobalLock(global) as *const u16;
            if p.is_null() {
                return None;
            }
            let max = GlobalSize(global) / 2;
            let units = std::slice::from_raw_parts(p, max);
            let end = units.iter().position(|&c| c == 0).unwrap_or(max);
            let s = String::from_utf16_lossy(&units[..end]);
            let _ = GlobalUnlock(global);
            Some(s)
        })
    };
    unsafe {
        let _ = CloseClipboard();
    }
    text
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Waits for the shortcut's Shift, Alt and Windows keys to be let go, so the
/// app in front sees Ctrl+C and not Ctrl+Shift+C.
fn wait_for_modifiers_up() {
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        let held = [VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN, VK_CONTROL]
            .iter()
            .any(|vk| unsafe { GetAsyncKeyState(vk.0 as i32) } < 0);
        if !held {
            return;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// Saves the clipboard, sends Ctrl+C, reads the text and puts the clipboard
/// back. If the app copied nothing, the clipboard was never touched.
fn from_clipboard() -> Option<String> {
    let saved = save()?;
    let before = unsafe { GetClipboardSequenceNumber() };
    wait_for_modifiers_up();
    let inputs = [
        key(VK_CONTROL, false),
        key(VK_C, false),
        key(VK_C, true),
        key(VK_CONTROL, true),
    ];
    if unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) } != inputs.len() as u32 {
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(400);
    while unsafe { GetClipboardSequenceNumber() } == before && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if unsafe { GetClipboardSequenceNumber() } == before {
        return None;
    }
    std::thread::sleep(Duration::from_millis(30));
    let text = read_text();
    restore(saved);
    text.and_then(|t| tidy(&t))
}

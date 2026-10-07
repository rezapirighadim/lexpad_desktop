//! macOS: the Accessibility API first, the clipboard second.
//!
//! Both need the Accessibility permission (reading another app's focused
//! element, and posting ⌘C to it). Without it nothing is read and the popup
//! offers the type-a-word box with a button to the right Settings pane.

#![allow(non_upper_case_globals)]

use std::ffi::c_void;
use std::time::{Duration, Instant};

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGRect;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting, NSWorkspace,
};
use objc2_foundation::{NSArray, NSData, NSString};

use super::{tidy, window_around, Capture, Permission, Rect, Via, CONTEXT_EACH_SIDE};

type AXUIElementRef = *const c_void;
type AXValueRef = *const c_void;
type AXError = i32;
const kAXErrorSuccess: AXError = 0;
const kAXValueTypeCGRect: u32 = 3;
const kAXValueTypeCFRange: u32 = 4;
/// The virtual key code of C on an ANSI keyboard (kVK_ANSI_C).
const KEY_C: u16 = 8;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct CFRange {
    location: isize,
    length: isize,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> AXError;
    fn AXValueGetValue(value: AXValueRef, kind: u32, out: *mut c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceFlagsState(state: i32) -> u64;
}

pub fn permission() -> Permission {
    if unsafe { AXIsProcessTrusted() } {
        Permission::Granted
    } else {
        Permission::Missing
    }
}

pub fn request_permission() -> bool {
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const c_void) }
}

/// The app in front, by its display name, unless it is this app.
fn frontmost_app() -> Option<String> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    if app.processIdentifier() as u32 == std::process::id() {
        return None;
    }
    app.localizedName().map(|n| n.to_string())
}

pub fn capture() -> Capture {
    let app = frontmost_app();
    if permission() != Permission::Granted {
        return Capture::empty(Permission::Missing, app);
    }
    if let Some(mut c) = from_accessibility() {
        c.app = app;
        return c;
    }
    match from_clipboard() {
        Some(text) => Capture {
            text: Some(text),
            context: None,
            app,
            permission: Permission::Granted,
            via: Via::Clipboard,
            anchor: None,
        },
        None => Capture::empty(Permission::Granted, app),
    }
}

/// An owned CF object, released on drop.
struct Owned(CFTypeRef);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

fn attribute(element: AXUIElementRef, name: &str) -> Option<Owned> {
    let name = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let err =
        unsafe { AXUIElementCopyAttributeValue(element, name.as_concrete_TypeRef(), &mut value) };
    (err == kAXErrorSuccess && !value.is_null()).then_some(Owned(value))
}

fn as_string(v: &Owned) -> Option<String> {
    unsafe {
        if core_foundation::base::CFGetTypeID(v.0) != core_foundation::string::CFString::type_id() {
            return None;
        }
        Some(CFString::wrap_under_get_rule(v.0 as CFStringRef).to_string())
    }
}

fn from_accessibility() -> Option<Capture> {
    let system = Owned(unsafe { AXUIElementCreateSystemWide() });
    // A hung app must not hang the shortcut.
    unsafe { AXUIElementSetMessagingTimeout(system.0, 0.4) };
    let focused = attribute(system.0, "AXFocusedUIElement")?;
    // Never read a password field.
    if let Some(role) = attribute(focused.0, "AXSubrole").and_then(|r| as_string(&r)) {
        if role == "AXSecureTextField" {
            return None;
        }
    }
    let text = attribute(focused.0, "AXSelectedText")
        .and_then(|v| as_string(&v))
        .and_then(|t| tidy(&t))?;

    let range = attribute(focused.0, "AXSelectedTextRange");
    let mut context = None;
    let mut anchor = None;
    if let Some(range) = &range {
        let mut r = CFRange::default();
        if unsafe {
            AXValueGetValue(
                range.0,
                kAXValueTypeCFRange,
                &mut r as *mut CFRange as *mut c_void,
            )
        } {
            if let Some(all) = attribute(focused.0, "AXValue").and_then(|v| as_string(&v)) {
                // CFRange counts UTF-16 units; turn it into characters.
                let units: Vec<u16> = all.encode_utf16().collect();
                let loc = (r.location.max(0) as usize).min(units.len());
                let len = (r.length.max(0) as usize).min(units.len() - loc);
                let before = String::from_utf16_lossy(&units[..loc]).chars().count();
                let sel = String::from_utf16_lossy(&units[loc..loc + len])
                    .chars()
                    .count();
                let chars: Vec<char> = all.chars().collect();
                context = Some(window_around(&chars, before, sel, CONTEXT_EACH_SIDE));
            }
            anchor = bounds(focused.0, range.0);
        }
    }
    Some(Capture {
        text: Some(text),
        context,
        app: None,
        permission: Permission::Granted,
        via: Via::Accessibility,
        anchor,
    })
}

/// Where the selected range is on screen, in global top-left points.
fn bounds(element: AXUIElementRef, range: CFTypeRef) -> Option<Rect> {
    let name = CFString::new("AXBoundsForRange");
    let mut value: CFTypeRef = std::ptr::null();
    let err = unsafe {
        AXUIElementCopyParameterizedAttributeValue(
            element,
            name.as_concrete_TypeRef(),
            range,
            &mut value,
        )
    };
    if err != kAXErrorSuccess || value.is_null() {
        return None;
    }
    let value = Owned(value);
    let mut rect = CGRect::default();
    if !unsafe {
        AXValueGetValue(
            value.0,
            kAXValueTypeCGRect,
            &mut rect as *mut CGRect as *mut c_void,
        )
    } {
        return None;
    }
    // Some apps answer with an empty rectangle at the origin.
    if rect.size.width <= 0.0 && rect.size.height <= 0.0 {
        return None;
    }
    Some(Rect {
        x: rect.origin.x,
        y: rect.origin.y,
        w: rect.size.width,
        h: rect.size.height,
    })
}

/// Every item on the pasteboard with every type it carries, as bytes.
type Saved = Vec<Vec<(Retained<NSString>, Retained<NSData>)>>;

fn save(pb: &NSPasteboard) -> Saved {
    let mut out = Vec::new();
    let Some(items) = pb.pasteboardItems() else {
        return out;
    };
    for item in items.iter() {
        let mut kept = Vec::new();
        for ty in item.types().iter() {
            if let Some(data) = item.dataForType(&ty) {
                kept.push((ty.clone(), data));
            }
        }
        out.push(kept);
    }
    out
}

fn restore(pb: &NSPasteboard, saved: Saved) {
    pb.clearContents();
    if saved.is_empty() {
        return;
    }
    let mut objects: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = Vec::new();
    for kept in saved {
        let item = NSPasteboardItem::new();
        for (ty, data) in kept {
            item.setData_forType(&data, &ty);
        }
        objects.push(ProtocolObject::from_retained(item));
    }
    let array = NSArray::from_retained_slice(&objects);
    pb.writeObjects(&array);
}

/// Waits until the shortcut's own keys are let go, so the ⌘C we send is not
/// read as ⌘⇧C by the app in front. Gives up after half a second.
fn wait_for_modifiers_up() {
    const SHIFT: u64 = 0x0002_0000;
    const CONTROL: u64 = 0x0004_0000;
    const OPTION: u64 = 0x0008_0000;
    const COMMAND: u64 = 0x0010_0000;
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        // 1 = kCGEventSourceStateHIDSystemState
        let flags = unsafe { CGEventSourceFlagsState(1) };
        if flags & (SHIFT | CONTROL | OPTION | COMMAND) == 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn send_copy() -> bool {
    let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
        return false;
    };
    let (Ok(down), Ok(up)) = (
        CGEvent::new_keyboard_event(source.clone(), KEY_C, true),
        CGEvent::new_keyboard_event(source, KEY_C, false),
    ) else {
        return false;
    };
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);
    up.post(CGEventTapLocation::HID);
    true
}

/// Saves the clipboard, sends ⌘C, reads the text, and puts the clipboard
/// back exactly: every item with every type it had. If the app copied
/// nothing, the clipboard was never touched and is left alone.
fn from_clipboard() -> Option<String> {
    let pb = NSPasteboard::generalPasteboard();
    let saved = save(&pb);
    let before = pb.changeCount();
    wait_for_modifiers_up();
    if !send_copy() {
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(400);
    while pb.changeCount() == before && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if pb.changeCount() == before {
        return None;
    }
    // Let a slow app finish writing every type before reading.
    std::thread::sleep(Duration::from_millis(30));
    let text = pb
        .stringForType(unsafe { NSPasteboardTypeString })
        .map(|s| s.to_string());
    restore(&pb, saved);
    text.and_then(|t| tidy(&t))
}

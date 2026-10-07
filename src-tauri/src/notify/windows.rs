//! Windows: toasts through the WinRT notification API, filed under the
//! app's AppUserModelID (`app.lexpad.desktop`, the identifier the installer
//! gives the Start menu shortcut). Reminders are scheduled toasts, so the
//! system delivers them even while Lexpad's window is shut. A click starts
//! Lexpad through its shortcut; the running copy then opens its window
//! (single instance), on Today.

use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::DateTime;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::UI::Notifications::{
    NotificationSetting, ScheduledToastNotification, ToastNotification, ToastNotificationManager,
    ToastNotifier,
};

use super::{Access, Planned};

/// The app's AppUserModelID: `identifier` in tauri.conf.json.
const AUMID: &str = "app.lexpad.desktop";
/// Seconds from 1601-01-01 (Windows time) to 1970-01-01 (Unix time).
const EPOCH_GAP: i64 = 11_644_473_600;
/// A scheduled toast's id holds at most 16 characters; ours start with this.
const REMINDER_PREFIX: &str = "lexpad-r";

fn notifier() -> Option<ToastNotifier> {
    // The calling thread may not have joined COM yet; joining twice is harmless.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID)).ok()
}

pub fn access() -> Access {
    match notifier().and_then(|n| n.Setting().ok()) {
        Some(NotificationSetting::Enabled) => Access::Granted,
        Some(_) => Access::Denied,
        None => Access::Unsupported,
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn toast_xml(title: &str, body: &str) -> Option<XmlDocument> {
    let doc = XmlDocument::new().ok()?;
    let xml = format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        escape(title),
        escape(body)
    );
    doc.LoadXml(&HSTRING::from(xml)).ok()?;
    Some(doc)
}

pub fn replace_reminders(plan: &[Planned]) {
    let Some(n) = notifier() else {
        return;
    };
    if let Ok(scheduled) = n.GetScheduledToastNotifications() {
        for s in scheduled {
            if s.Id()
                .is_ok_and(|id| id.to_string().starts_with(REMINDER_PREFIX))
            {
                let _ = n.RemoveFromSchedule(&s);
            }
        }
    }
    for p in plan {
        let Some(xml) = toast_xml(&p.title, &p.body) else {
            continue;
        };
        let when = DateTime {
            UniversalTime: (p.at.timestamp() + EPOCH_GAP) * 10_000_000,
        };
        if let Ok(s) = ScheduledToastNotification::CreateScheduledToastNotification(&xml, when) {
            let _ = s.SetId(&HSTRING::from(format!("{REMINDER_PREFIX}{}", p.id)));
            if let Err(e) = n.AddToSchedule(&s) {
                log::warn!("could not schedule a reminder: {e}");
            }
        }
    }
}

pub fn show_now(_key: &str, title: &str, body: &str) {
    let (Some(n), Some(xml)) = (notifier(), toast_xml(title, body)) else {
        return;
    };
    if let Ok(t) = ToastNotification::CreateToastNotification(&xml) {
        if let Err(e) = n.Show(&t) {
            log::warn!("could not show a notification: {e}");
        }
    }
}

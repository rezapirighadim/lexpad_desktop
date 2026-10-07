//! Notifications on this computer, shown by the system itself (macOS
//! Notification Center, Windows toasts).
//!
//! Two kinds, as on the phones:
//!
//! - **The daily practice reminder** stays local. Lexpad's window works out
//!   the plan exactly as the phone apps do (`sync/reminder.ts`: the account's
//!   reminder time, the real number of words due, the streak and come-back
//!   nudges, a week ahead) and hands it to the core (`schedule_reminders`),
//!   which gives it to the system to deliver at those times, even while the
//!   window is shut. Nothing here invents a count: every word in a reminder
//!   is the page's.
//! - **What the server says** (announcements, gifts, a connected assistant)
//!   reaches phones through Firebase, which does not deliver to desktop apps.
//!   The core reads the account's inbox (`GET /notifications`, the bell on
//!   Today) when it starts, when Lexpad's window opens, and every
//!   `INBOX_EVERY` while it runs, and shows what `pick` lets through: unread,
//!   newer than this computer's first look and than `INBOX_FRESH`, not shown
//!   here before, never a come-back note (the local plan carries those),
//!   product news only when the account takes it, and nothing the server
//!   says was not meant to pop up or has already popped up on a phone or in
//!   a browser (`popUp`, `poppedAt`, when the API sends them). Nothing is
//!   marked read: that is the bell's job, as with a phone's pop-up.
//!
//! A click opens Lexpad's window (on Today, or the message's own in-app
//! path). The switch "Notifications on this computer" (the desktop section
//! of Settings) silences both kinds here without touching the account's
//! reminder on other devices.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// How often the inbox is read while the app runs. Announcements and gifts
/// are not urgent; half an hour keeps the API quiet and still feels prompt.
pub const INBOX_EVERY: Duration = Duration::from_secs(30 * 60);
/// The oldest message shown as a pop-up: older news is for the bell.
pub const INBOX_FRESH_HOURS: i64 = 72;
/// At most this many pop-ups from one read, so a computer back from a week
/// away does not shower the screen.
pub const INBOX_MAX_PER_READ: usize = 3;
/// How many shown ids are remembered: well beyond one read's worth.
pub const SHOWN_MAX: usize = 200;
/// The reminder ids the web app uses (`REMINDER_IDS`: 1 to 20).
pub const REMINDER_IDS: std::ops::RangeInclusive<u32> = 1..=20;
/// Bounds on a reminder's text, as the API bounds an inbox message.
const TITLE_MAX: usize = 200;
const BODY_MAX: usize = 1000;

/// Whether this app may show notifications.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Granted,
    Denied,
    /// Not asked yet (macOS).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    NotDetermined,
    /// The system cannot show them here (a build run outside its bundle).
    Unsupported,
}

/// One reminder as the window plans it (`Notice` in the web app).
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Notice {
    pub id: u32,
    pub title: String,
    pub body: String,
    /// When it fires, as the page serialises a Date (ISO 8601, UTC).
    pub at: String,
}

/// A reminder ready for the system.
#[derive(Clone, Debug, PartialEq)]
pub struct Planned {
    pub id: u32,
    pub title: String,
    pub body: String,
    pub at: DateTime<Utc>,
}

/// The identifier a reminder is filed under, so a new plan replaces it.
// A click comes back with its identifier on macOS; Windows starts the app instead.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn reminder_key(id: u32) -> String {
    format!("lexpad.reminder.{id}")
}

/// The identifier an inbox message is shown under.
pub fn inbox_key(id: &str) -> String {
    format!("lexpad.inbox.{id}")
}

/// Checks a plan from the window: ids in the web app's block, text within
/// bounds and free of control characters, times that parse and lie ahead.
/// Anything else is dropped, never shown.
pub fn plan(notices: &[Notice], now: DateTime<Utc>) -> Vec<Planned> {
    let mut out: Vec<Planned> = notices
        .iter()
        .filter(|n| REMINDER_IDS.contains(&n.id))
        .filter_map(|n| {
            let at = DateTime::parse_from_rfc3339(&n.at)
                .ok()?
                .with_timezone(&Utc);
            let clean = |s: &str, max: usize| -> Option<String> {
                let s: String = s
                    .chars()
                    .filter(|c| !c.is_control() || *c == '\n')
                    .collect();
                let s = s.trim();
                (!s.is_empty() && s.chars().count() <= max).then(|| s.to_owned())
            };
            (at > now).then_some(Planned {
                id: n.id,
                title: clean(&n.title, TITLE_MAX)?,
                body: clean(&n.body, BODY_MAX)?,
                at,
            })
        })
        .collect();
    out.sort_by_key(|p| p.at);
    out.dedup_by_key(|p| p.id);
    out.truncate(REMINDER_IDS.count());
    out
}

/// One message from the account's inbox, as the API sends it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub link: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub read_at: Option<String>,
    /// Whether the server meant it to pop up at all (absent before the API
    /// sends it: then the account's product-news switch decides).
    #[serde(default)]
    pub pop_up: Option<bool>,
    /// When a phone or a browser already showed it (absent: not known).
    #[serde(default)]
    pub popped_at: Option<String>,
}

/// What this computer remembers about the inbox (in `settings.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct InboxMemory {
    /// The moment of this computer's first look: nothing older pops up, so
    /// connecting does not replay the past.
    pub since: Option<String>,
    /// Ids already shown here, newest last.
    pub shown: Vec<String>,
}

/// The messages to show now, oldest first, and the memory after showing them.
pub fn pick(
    items: &[InboxItem],
    memory: &InboxMemory,
    push_news: bool,
    now: DateTime<Utc>,
) -> (Vec<InboxItem>, InboxMemory) {
    let since = memory
        .since
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc));
    let Some(since) = since else {
        // The first look: remember when, show nothing.
        return (
            Vec::new(),
            InboxMemory {
                since: Some(now.to_rfc3339()),
                shown: memory.shown.clone(),
            },
        );
    };
    let fresh_from = now - chrono::Duration::hours(INBOX_FRESH_HOURS);
    let mut chosen: Vec<InboxItem> = items
        .iter()
        .filter(|n| n.read_at.is_none())
        .filter(|n| n.popped_at.is_none())
        .filter(|n| n.pop_up != Some(false))
        .filter(|n| n.kind != "comeback")
        .filter(|n| match n.kind.as_str() {
            // A gift is about the account itself and always pops up, as on
            // the phones; news only for an account that takes it.
            "gift_ai_cards" | "gift_pro" | "connection" => true,
            _ => push_news,
        })
        .filter(|n| !memory.shown.contains(&n.id))
        .filter(|n| {
            DateTime::parse_from_rfc3339(&n.created_at)
                .map(|d| {
                    let d = d.with_timezone(&Utc);
                    d > since && d > fresh_from && d <= now + chrono::Duration::minutes(5)
                })
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    chosen.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    // Every one of them counts as seen here; only the newest few pop up,
    // and the rest wait in the bell rather than trickling out later.
    let mut shown = memory.shown.clone();
    shown.extend(chosen.iter().map(|n| n.id.clone()));
    if chosen.len() > INBOX_MAX_PER_READ {
        chosen.drain(..chosen.len() - INBOX_MAX_PER_READ);
    }
    if shown.len() > SHOWN_MAX {
        shown.drain(..shown.len() - SHOWN_MAX);
    }
    (
        chosen,
        InboxMemory {
            since: memory.since.clone(),
            shown,
        },
    )
}

/// Where a click on a notification goes: its own in-app path when it is a
/// plain one, Today otherwise.
// A click comes back with its identifier on macOS; Windows starts the app instead.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn click_path(link: Option<&str>) -> String {
    match link {
        Some(p)
            if p.starts_with('/')
                && !p.starts_with("//")
                && !p.contains('\\')
                && p.len() <= 512
                && !p.chars().any(|c| c.is_whitespace() || c.is_control()) =>
        {
            p.to_owned()
        }
        _ => "/".into(),
    }
}

/* ------------------------------------------------------- the system */

/// Links of the messages shown this run, by notification identifier.
static LINKS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// What a click on the notification filed under `key` opens.
// A click comes back with its identifier on macOS; Windows starts the app instead.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn path_for(key: &str) -> String {
    let link = LINKS
        .lock()
        .ok()
        .and_then(|m| m.as_ref().and_then(|m| m.get(key).cloned()));
    click_path(link.as_deref())
}

/// Wires the system's click handler. Call once, on the main thread.
pub fn init(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    macos::init(app);
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Whether notifications may be shown, without asking.
pub fn access() -> Access {
    #[cfg(target_os = "macos")]
    return macos::access();
    #[cfg(windows)]
    return windows::access();
    #[cfg(not(any(target_os = "macos", windows)))]
    return Access::Unsupported;
}

/// Asks (macOS shows its own prompt once) and says what came of it.
pub fn request() -> Access {
    #[cfg(target_os = "macos")]
    return macos::request();
    #[cfg(not(target_os = "macos"))]
    return access();
}

/// The system's settings page for this app's notifications.
pub fn settings_url() -> &'static str {
    if cfg!(target_os = "macos") {
        "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=app.lexpad.desktop"
    } else {
        "ms-settings:notifications"
    }
}

/// Replaces every reminder the system holds for this app with `plan`.
pub fn replace_reminders(plan: &[Planned]) {
    #[cfg(target_os = "macos")]
    macos::replace_reminders(plan);
    #[cfg(windows)]
    windows::replace_reminders(plan);
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = plan;
}

/// Shows one message now.
fn show_now(key: &str, title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    macos::show_now(key, title, body);
    #[cfg(windows)]
    windows::show_now(key, title, body);
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (key, title, body);
}

/// Whether this computer shows notifications at all (the desktop switch).
fn enabled(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .and_then(|s| s.settings.lock().ok().map(|s| s.notifications))
        .unwrap_or(false)
}

/// Reads the inbox once and shows what `pick` lets through.
pub async fn read_inbox(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if state.api.user().await.is_none() || !enabled(app) || access() != Access::Granted {
        return;
    }
    let Ok(page) = state
        .api
        .request(reqwest::Method::GET, "/notifications?limit=20", None, false)
        .await
    else {
        return;
    };
    let items: Vec<InboxItem> = page
        .get("notifications")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    // The account's product-news switch; unknown counts as off, so news
    // never pops up for somebody who may have said no.
    let push_news = state
        .api
        .request(reqwest::Method::GET, "/settings", None, false)
        .await
        .ok()
        .and_then(|s| s.get("pushNews").and_then(Value::as_bool))
        .unwrap_or(false);
    let memory = state
        .settings
        .lock()
        .map(|s| s.inbox.clone())
        .unwrap_or_default();
    let (show, next) = pick(&items, &memory, push_news, Utc::now());
    if next != memory {
        if let Err(e) = state.update_settings(|s| s.inbox = next.clone()) {
            log::warn!("could not remember the inbox: {e}");
            return;
        }
    }
    for n in show {
        let key = inbox_key(&n.id);
        if let Ok(mut links) = LINKS.lock() {
            links
                .get_or_insert_with(HashMap::new)
                .insert(key.clone(), n.link.clone().unwrap_or_default());
        }
        show_now(&key, &n.title, &n.body);
    }
}

/// Reads the inbox now and then every `INBOX_EVERY` while the app runs.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Let the app finish starting first.
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            read_inbox(&app).await;
            tokio::time::sleep(INBOX_EVERY).await;
        }
    });
}

/// Turns notifications on this computer off: the reminders the system
/// holds are taken back. (On again, the window hands over a new plan the
/// next time it plans, which it does whenever it opens.)
pub fn silence() {
    replace_reminders(&[]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn notice(id: u32, at: &str) -> Notice {
        Notice {
            id,
            title: "Lexpad".into(),
            body: "6 words are waiting".into(),
            at: at.into(),
        }
    }

    #[test]
    fn a_plan_keeps_only_future_well_formed_reminders_in_the_apps_ids() {
        let now = at("2026-10-07T12:00:00Z");
        let p = plan(
            &[
                notice(1, "2026-10-07T18:00:00.000Z"),
                notice(2, "2026-10-08T18:00:00.000Z"),
                notice(3, "2026-10-07T11:00:00.000Z"), // already past
                notice(21, "2026-10-09T18:00:00.000Z"), // not ours
                notice(0, "2026-10-09T18:00:00.000Z"),
                notice(4, "tomorrow"),
                Notice {
                    id: 5,
                    title: "".into(),
                    body: "x".into(),
                    at: "2026-10-10T18:00:00Z".into(),
                },
                Notice {
                    id: 6,
                    title: "T".repeat(201),
                    body: "x".into(),
                    at: "2026-10-10T18:00:00Z".into(),
                },
            ],
            now,
        );
        assert_eq!(p.iter().map(|n| n.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(p[0].body, "6 words are waiting");
    }

    #[test]
    fn a_plan_never_holds_more_than_the_app_can_name() {
        let now = at("2026-10-07T00:00:00Z");
        let many: Vec<_> = (1..=20)
            .chain(1..=20)
            .map(|i| notice(i, &format!("2026-10-{:02}T18:00:00Z", 8 + i % 20)))
            .collect();
        let p = plan(&many, now);
        assert!(p.len() <= 20);
        let mut ids: Vec<_> = p.iter().map(|n| n.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), p.len(), "one per id");
    }

    #[test]
    fn control_characters_never_reach_the_system() {
        let now = at("2026-10-07T00:00:00Z");
        let p = plan(
            &[Notice {
                id: 1,
                title: "Lex\u{7}pad".into(),
                body: "a\u{1b}[31mb".into(),
                at: "2026-10-08T00:00:00Z".into(),
            }],
            now,
        );
        assert_eq!(p[0].title, "Lexpad");
        assert_eq!(p[0].body, "a[31mb");
    }

    fn item(id: &str, kind: &str, created: &str) -> InboxItem {
        InboxItem {
            id: id.into(),
            kind: kind.into(),
            title: "t".into(),
            body: "b".into(),
            link: None,
            created_at: created.into(),
            read_at: None,
            pop_up: None,
            popped_at: None,
        }
    }

    #[test]
    fn the_first_look_shows_nothing_and_remembers_when() {
        let now = at("2026-10-07T12:00:00Z");
        let items = [item("a", "gift_pro", "2026-10-07T11:00:00Z")];
        let (show, mem) = pick(&items, &InboxMemory::default(), true, now);
        assert!(show.is_empty());
        assert_eq!(mem.since.as_deref(), Some("2026-10-07T12:00:00+00:00"));
    }

    #[test]
    fn new_unread_messages_pop_up_once() {
        let mem = InboxMemory {
            since: Some("2026-10-07T00:00:00Z".into()),
            shown: vec![],
        };
        let now = at("2026-10-07T12:00:00Z");
        let items = [
            item("old", "campaign", "2026-10-06T23:00:00Z"), // before the first look
            item("new", "campaign", "2026-10-07T11:00:00Z"),
            item("gift", "gift_ai_cards", "2026-10-07T10:00:00Z"),
        ];
        let (show, mem) = pick(&items, &mem, true, now);
        assert_eq!(
            show.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            ["gift", "new"]
        );
        // The next read shows nothing again.
        let (again, _) = pick(&items, &mem, true, now);
        assert!(again.is_empty());
    }

    #[test]
    fn opt_outs_read_messages_come_back_and_what_already_popped_are_left_to_the_bell() {
        let mem = InboxMemory {
            since: Some("2026-10-01T00:00:00Z".into()),
            shown: vec![],
        };
        let now = at("2026-10-07T12:00:00Z");
        let mut read = item("read", "gift_pro", "2026-10-07T11:00:00Z");
        read.read_at = Some("2026-10-07T11:30:00Z".into());
        let mut popped = item("popped", "gift_pro", "2026-10-07T11:00:00Z");
        popped.popped_at = Some("2026-10-07T11:00:01Z".into());
        let mut quiet = item("quiet", "campaign", "2026-10-07T11:00:00Z");
        quiet.pop_up = Some(false);
        let items = [
            read,
            popped,
            quiet,
            item("news", "campaign", "2026-10-07T11:00:00Z"),
            item("comeback", "comeback", "2026-10-07T11:00:00Z"),
            item("stale", "gift_pro", "2026-10-03T11:00:00Z"), // older than three days
            item("gift", "gift_pro", "2026-10-07T11:00:00Z"),
        ];
        // Product news off: only the gift.
        let (show, _) = pick(&items, &mem, false, now);
        assert_eq!(
            show.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            ["gift"]
        );
        // On: the news too, never the come-back, the quiet, the read or the popped.
        let (show, _) = pick(&items, &mem, true, now);
        let mut ids: Vec<_> = show.iter().map(|n| n.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["gift", "news"]);
    }

    #[test]
    fn a_crowd_is_cut_to_the_newest_few_and_the_memory_stays_bounded() {
        let mut mem = InboxMemory {
            since: Some("2026-10-01T00:00:00Z".into()),
            shown: (0..SHOWN_MAX).map(|i| format!("old{i}")).collect(),
        };
        let now = at("2026-10-07T12:00:00Z");
        let items: Vec<_> = (0..8)
            .map(|i| {
                item(
                    &format!("n{i}"),
                    "gift_pro",
                    &format!("2026-10-07T0{i}:00:00Z"),
                )
            })
            .collect();
        let (show, next) = pick(&items, &mem, true, now);
        assert_eq!(
            show.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
            ["n5", "n6", "n7"]
        );
        assert_eq!(next.shown.len(), SHOWN_MAX);
        assert_eq!(next.shown.last().map(String::as_str), Some("n7"));
        mem = next;
        assert!(pick(&items, &mem, true, now).0.is_empty());
    }

    #[test]
    fn a_click_opens_a_plain_in_app_path_or_today() {
        assert_eq!(click_path(Some("/settings/plan")), "/settings/plan");
        assert_eq!(click_path(None), "/");
        assert_eq!(click_path(Some("")), "/");
        assert_eq!(click_path(Some("https://evil.example/")), "/");
        assert_eq!(click_path(Some("//evil.example/")), "/");
        assert_eq!(click_path(Some("/a b")), "/");
    }
}

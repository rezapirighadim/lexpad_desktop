//! The Lexpad Player's window (spec `uiux/specs/2026-10-25-video-player.md`
//! §2, §13, §24.3): its own dark window, the web app's `/player` route from
//! the same bundled build as Lexpad's window, with the same configuration
//! script, navigation lock and session rule (the page holds no session; the
//! core does). It plays what the media engine (`media/`) serves it.
//!
//! Frameless with the system's window buttons: on macOS the traffic lights
//! sit inset at (18, 18) over the page and hide with the player's chrome;
//! on Windows the page draws the caption buttons. Default size 72% of the
//! work area at 16:9, centred on the monitor under the pointer; its place is
//! remembered. The mini player is this same window made small and kept on
//! top (one decoder, one stream, nothing to keep in step), and back again.
//!
//! On Windows the Player has a taskbar button of its own: its own
//! AppUserModelID (`Lexpad.Player`), relaunching `Lexpad.exe --player`.

use std::sync::Mutex;

use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt as _;

use crate::main_window;
use crate::placement::{self, Area};
use crate::settings::WindowPlace;
use crate::state::AppState;

pub const LABEL: &str = "player";
/// The window's title (the page sets the video's title once one plays).
const TITLE: &str = "Lexpad Player";
/// The smallest the window may be (§2): a 16:9 picture still has room.
const MIN_SIZE: (f64, f64) = (480.0, 270.0);
/// The first size: this share of the work area, at 16:9 (§2).
const WORK_SHARE: f64 = 0.72;
/// The mini player (§13): 448×252, 16:9, 16 px from the screen's corner.
const MINI_SIZE: (f64, f64) = (448.0, 252.0);
const MINI_MIN: (f64, f64) = (320.0, 180.0);
const MINI_MARGIN: f64 = 16.0;
/// The Player's own taskbar identity on Windows (§24.3).
#[cfg_attr(not(windows), allow(dead_code))]
pub const APP_USER_MODEL_ID: &str = "Lexpad.Player";
/// The flag a pinned Player's taskbar button starts Lexpad with.
pub const PLAYER_FLAG: &str = "--player";

/// How the window was opened, which the page reads from its address: from
/// Lexpad's own window or the tray ("app": no first-open sign-in), with a
/// file ("file": it plays at once), to show the open dialog ("pick"), or
/// by itself at launch ("launch").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    App,
    // With `player-dev`, files open in the development page instead.
    #[cfg_attr(feature = "player-dev", allow(dead_code))]
    File,
    Pick,
    Launch,
}

/// Which door opened the Player, for the activity reports (spec
/// `uiux/2026-10-31-player-next/spec.md` A1): the page sends it as
/// `player.window {from}`. One list, kept the same as the page's
/// (lexpad_front `features/player/door.ts`) and the server's allow-list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    /// The Start window's "Open a video…".
    Start,
    /// The launch table (`welcome::first_window`), a second launch, the Dock,
    /// `--player` (the Start menu's "Lexpad Player").
    Launch,
    /// The tray panel's "Watch a video…" row and the tray menu's "Open a video…".
    Tray,
    /// The learning window's Player page.
    Watch,
    /// Today's "Learn from what you watch" card.
    TodayCard,
    /// Today's "Your first 5 words" row about the Player.
    FirstFive,
    /// The shortcut card's hint when a video app is in front.
    PopupHint,
    /// A file dropped on the learning window or the Start window.
    Drop,
    /// The system's "Open with", a file association, a file in the arguments.
    File,
    /// The menu bar (⌘O, Window → Player ⌘2) or Ctrl+2 on Windows.
    Menu,
}

impl Door {
    pub const ALL: [Door; 10] = [
        Door::Start,
        Door::Launch,
        Door::Tray,
        Door::Watch,
        Door::TodayCard,
        Door::FirstFive,
        Door::PopupHint,
        Door::Drop,
        Door::File,
        Door::Menu,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Door::Start => "start",
            Door::Launch => "launch",
            Door::Tray => "tray",
            Door::Watch => "watch",
            Door::TodayCard => "today_card",
            Door::FirstFive => "first_five",
            Door::PopupHint => "popup_hint",
            Door::Drop => "drop",
            Door::File => "file",
            Door::Menu => "menu",
        }
    }

    pub fn parse(s: &str) -> Option<Door> {
        Door::ALL.into_iter().find(|d| d.as_str() == s)
    }

    /// The doors the learning window's page may name in `open_player`: its
    /// own buttons and keys. The others are the core's to say.
    fn page_may_name(self) -> bool {
        matches!(
            self,
            Door::Watch | Door::TodayCard | Door::FirstFive | Door::Menu
        )
    }
}

/// The door a page named in `open_player`: absent is none, anything that is
/// not one of the page's doors is refused.
pub fn page_door(from: Option<&str>) -> Result<Option<Door>, String> {
    match from {
        None => Ok(None),
        Some(f) => Door::parse(f)
            .filter(|d| d.page_may_name())
            .map(Some)
            .ok_or_else(|| "not a door".to_string()),
    }
}

/// The event that tells an open Player which door brought it forward.
pub const DOOR_EVENT: &str = "player:door";

impl Opened {
    fn from(self) -> Option<&'static str> {
        match self {
            Opened::App => Some("app"),
            Opened::File => Some("file"),
            Opened::Pick => Some("pick"),
            Opened::Launch => None,
        }
    }

    /// The route's query: how it was opened, and `seen=start` once the
    /// Start window has been shown, so the Player never asks for a sign-in
    /// of its own after it (Start window spec §7).
    fn query(self, start_seen: bool, door: Option<Door>) -> String {
        let parts: Vec<String> = self
            .from()
            .map(|f| format!("from={f}"))
            .into_iter()
            .chain(start_seen.then(|| "seen=start".to_string()))
            .chain(door.map(|d| format!("door={}", d.as_str())))
            .collect();
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    }
}

/// Where the big window was before it became the mini player.
static BEFORE_MINI: Mutex<Option<Area>> = Mutex::new(None);

/// Opens the Player, or brings it forward, through `door` (`None`: a second
/// step of one opening, which must not be counted twice).
pub fn open(app: &AppHandle, opened: Opened, door: Option<Door>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || open_now(&handle, opened, door));
}

fn open_now(app: &AppHandle, opened: Opened, door: Option<Door>) {
    crate::refresh_dock(app, true);
    if let Some(win) = app.get_webview_window(LABEL) {
        // Already in front (a menu command while watching): no door was used.
        let in_front = win.is_focused().unwrap_or(false)
            && win.is_visible().unwrap_or(false)
            && !win.is_minimized().unwrap_or(false);
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        if let Some(d) = door.filter(|_| !in_front) {
            let _ = win.emit_to(LABEL, DOOR_EVENT, d.as_str());
        }
        if opened == Opened::Pick {
            let _ = win.emit_to(LABEL, "player:menu", "open");
        }
        return;
    }
    if let Err(e) = build(app, opened, door) {
        log::warn!("could not open the Player: {e}");
        crate::refresh_dock(app, false);
    }
}

/// The script that runs before the page: the app's configuration (as for
/// Lexpad's window) and the Player's route, so the router starts there.
fn init_script(device_id: &str, opened: Opened, start_seen: bool, door: Option<Door>) -> String {
    format!(
        "{}\nif (!location.hash || location.hash === '#/') history.replaceState(null, '', '#/player{}');",
        main_window::config_script(device_id),
        opened.query(start_seen, door)
    )
}

fn build(app: &AppHandle, opened: Opened, door: Option<Door>) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let (device_id, saved, start_seen) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| tauri::Error::FailedToReceiveMessage)?;
        (s.device_id.clone(), s.player_window, s.start_seen)
    };
    let nav = app.clone();
    let popups = app.clone();
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title(TITLE)
        .inner_size(1280.0, 720.0)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .visible(false)
        .background_color(tauri::window::Color(0, 0, 0, 255))
        .initialization_script(init_script(&device_id, opened, start_seen, door))
        .on_navigation(move |url| {
            if main_window::is_app_page(url) {
                return true;
            }
            open_outside(&nav, url);
            false
        })
        .on_new_window(move |url, _features| {
            open_outside(&popups, &url);
            NewWindowResponse::Deny
        });
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(18.0, 18.0));
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    let win = builder.build()?;
    restore(app, &win, saved);
    #[cfg(windows)]
    taskbar::own_button(&win);
    #[cfg(windows)]
    taskbar::no_border(&win);
    win.show()?;
    win.set_focus()?;
    Ok(())
}

fn open_outside(app: &AppHandle, url: &Url) {
    if main_window::may_open_outside(url) {
        if let Err(e) = app.opener().open_url(url.as_str(), None::<&str>) {
            log::warn!("could not open a link: {e}");
        }
    }
}

/// The first place: where it was (kept inside the work area of its monitor),
/// or 72% of the work area under the pointer at 16:9, centred.
pub fn initial_place(
    screens: &[placement::Screen],
    pointer: Option<(f64, f64)>,
    saved: Option<WindowPlace>,
    logical: bool,
) -> Option<Area> {
    if let Some(p) = saved {
        let rect = Area::new(p.x, p.y, p.w, p.h);
        let screen = placement::screen_at(screens, rect.centre())?;
        return Some(placement::clamp(rect, screen.work, 0.0));
    }
    let screen = pointer
        .and_then(|p| placement::screen_at(screens, p))
        .or_else(|| screens.first().copied())?;
    let work = screen.work;
    let mut w = work.w * WORK_SHARE;
    let mut h = w * 9.0 / 16.0;
    if h > work.h * WORK_SHARE {
        h = work.h * WORK_SHARE;
        w = h * 16.0 / 9.0;
    }
    let u = screen.unit(logical);
    let (w, h) = (w.max(MIN_SIZE.0 * u), h.max(MIN_SIZE.1 * u));
    let (cx, cy) = work.centre();
    Some(Area::new(cx - w / 2.0, cy - h / 2.0, w, h))
}

fn restore(app: &AppHandle, win: &WebviewWindow, saved: Option<WindowPlace>) {
    let screens = placement::screens(app);
    let Some(rect) = initial_place(&screens, placement::pointer(app), saved, placement::LOGICAL)
    else {
        let _ = win.center();
        return;
    };
    if let Err(e) = placement::apply(win, rect) {
        log::warn!("could not place the Player: {e}");
    }
    if saved.is_some_and(|p| p.maximized) {
        let _ = win.maximize();
    }
}

/// Remembers where the window is, before it closes or the app quits (not
/// while it is the mini player: the big window's place is kept).
pub fn remember(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    if win.is_minimized().unwrap_or(false) || win.is_fullscreen().unwrap_or(false) {
        return;
    }
    let mini = BEFORE_MINI.lock().ok().and_then(|b| *b);
    let maximized = win.is_maximized().unwrap_or(false);
    let rect = match mini {
        Some(big) => big,
        None => match placement::window_rect(&win) {
            Some(r) => r,
            None => return,
        },
    };
    let place = WindowPlace {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        maximized,
    };
    if let Err(e) = app
        .state::<AppState>()
        .update_settings(|s| s.player_window = Some(place))
    {
        log::warn!("could not remember the Player's place: {e}");
    }
}

/// The mini player's place: the bottom-right corner of the work area, 16 px in.
pub fn mini_place(work: Area, unit: f64) -> Area {
    let (w, h) = (MINI_SIZE.0 * unit, MINI_SIZE.1 * unit);
    let m = MINI_MARGIN * unit;
    Area::new(work.right() - w - m, work.bottom() - h - m, w, h)
}

fn screen_of(app: &AppHandle, win: &WebviewWindow) -> Option<placement::Screen> {
    let rect = placement::window_rect(win)?;
    placement::screen_at(&placement::screens(app), rect.centre())
}

/* ------------------------------------------------------------ commands */

/// Lexpad's window, opened or brought forward, at an in-app path (a word's
/// page, a session, Settings → Video player); the path is checked as the
/// web app checks its own (`commands::app_path`).
#[tauri::command]
pub fn open_lexpad(app: AppHandle, path: Option<String>) -> Result<(), String> {
    let path = match path {
        Some(p) => {
            crate::commands::app_path(&p).ok_or("not an app path")?;
            Some(p)
        }
        None => None,
    };
    main_window::open(&app, path);
    Ok(())
}

/// A video the Player should play once it listens (a Continue watching card
/// on the Watch page): the engine's file id, never a path.
static PLAY: Mutex<Option<String>> = Mutex::new(None);

/// How long the Player's page has to finish closing (its `onCloseRequested`
/// saves where the video was, then `destroy()`s the window) before the core
/// closes the window itself, a page that hung included.
pub const CLOSE_FALLBACK: std::time::Duration = std::time::Duration::from_secs(2);

/// What the core does with a close request for the Player (`CloseGuard`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseStep {
    /// The first request: the page closes it; the core waits, with this ticket.
    Arm(u64),
    /// A second request while one is pending: the core closes it now.
    Destroy,
}

/// The core's side of closing the Player. The page registers a close
/// listener, so Tauri holds every close (button, ⌘W, Alt+F4, the mini
/// player's close, the system's) for the page to finish; if the page never
/// does, this closes the window: on a second request at once, or when
/// `CLOSE_FALLBACK` passes after the first. Pure, so it is tested alone.
#[derive(Debug, Default)]
pub struct CloseGuard {
    armed: Option<u64>,
    next: u64,
}

impl CloseGuard {
    pub const fn new() -> Self {
        Self {
            armed: None,
            next: 0,
        }
    }

    /// A close was asked for.
    pub fn request(&mut self) -> CloseStep {
        if self.armed.take().is_some() {
            CloseStep::Destroy
        } else {
            self.next += 1;
            self.armed = Some(self.next);
            CloseStep::Arm(self.next)
        }
    }

    /// The wait for `ticket` ran out: whether the core closes the window now
    /// (false when the window already went, or a newer request took over).
    pub fn timed_out(&mut self, ticket: u64) -> bool {
        if self.armed == Some(ticket) {
            self.armed = None;
            true
        } else {
            false
        }
    }

    /// The window is gone; a timer still running finds nothing to do.
    pub fn closed(&mut self) {
        self.armed = None;
    }
}

static CLOSING: Mutex<CloseGuard> = Mutex::new(CloseGuard::new());

fn destroy_player(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        log::warn!("the Player did not close by itself; closing it");
        let _ = win.destroy();
    }
}

/// A close request reached the Player (see `CloseGuard`).
pub fn close_requested(app: &AppHandle) {
    let step = CLOSING.lock().map(|mut g| g.request());
    match step {
        Ok(CloseStep::Destroy) => destroy_player(app),
        Ok(CloseStep::Arm(ticket)) => {
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(CLOSE_FALLBACK);
                if CLOSING.lock().is_ok_and(|mut g| g.timed_out(ticket)) {
                    let handle = app.clone();
                    let _ = app.run_on_main_thread(move || destroy_player(&handle));
                }
            });
        }
        Err(_) => destroy_player(app),
    }
}

/// How long quitting waits for the Player's page to close (it saves where
/// the video was and sends its last counts: the minutes watched since its
/// last report, the subtitle moves). Its own close is capped at about a
/// second (`closeAfter` in the page); this is the most a quit is delayed.
pub const QUIT_WAIT: std::time::Duration = std::time::Duration::from_millis(1_500);

/// Lexpad is quitting with the Player open (the tray's or the menu's Quit,
/// a shutdown): its page is asked to close first, as its close button would,
/// and the app quits with `code` once the window is gone or `QUIT_WAIT` has
/// passed. False when there is no Player: quit at once.
pub fn close_before_quit(app: &AppHandle, code: i32) -> bool {
    let Some(win) = app.get_webview_window(LABEL) else {
        return false;
    };
    if win.close().is_err() {
        return false;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while start.elapsed() < QUIT_WAIT && app.get_webview_window(LABEL).is_some() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        app.exit(code);
    });
    true
}

/// The Player's window is gone.
pub fn destroyed() {
    if let Ok(mut g) = CLOSING.lock() {
        g.closed();
    }
}

/// Whether `id` looks like an id the engine gave (never a path).
fn is_file_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The Player through `door`: to play `file_id`, to the open dialog with
/// `pick`, or just brought forward.
pub fn open_through(
    app: &AppHandle,
    pick: bool,
    file_id: Option<String>,
    door: Option<Door>,
) -> Result<(), String> {
    if let Some(id) = file_id {
        if !is_file_id(&id) {
            return Err("not a file id".into());
        }
        if app.get_webview_window(LABEL).is_some() {
            let _ = app.emit_to(LABEL, "player:play", id);
        } else if let Ok(mut p) = PLAY.lock() {
            *p = Some(id);
        }
        open(app, Opened::App, door);
        return Ok(());
    }
    open(app, if pick { Opened::Pick } else { Opened::App }, door);
    Ok(())
}

/// The Player, opened or brought forward (the Watch page, Today's card, ⌘2 /
/// Ctrl+2): to the open dialog with `pick`, or to play `file_id` (an id the
/// engine gave). `from` is the page's door (`Door::page_may_name`).
#[tauri::command]
pub fn open_player(
    app: AppHandle,
    pick: Option<bool>,
    file_id: Option<String>,
    from: Option<String>,
) -> Result<(), String> {
    let door = page_door(from.as_deref())?;
    open_through(&app, pick == Some(true), file_id, door)
}

/// The page asks once, when it starts listening, for a video it should play.
#[tauri::command]
pub fn player_take_play() -> Option<String> {
    PLAY.lock().ok().and_then(|mut p| p.take())
}

/* ------------------------------------------------------------ the right-click menu */

/// One item of the Player's right-click menu, as the page describes it. The
/// page owns the words and the keys (they come from its own key table); the
/// core draws the system's menu, so it looks native on each system and has
/// the same items on both.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CtxItem {
    Item {
        command: String,
        label: String,
        accel: Option<String>,
        enabled: Option<bool>,
    },
    Check {
        command: String,
        label: String,
        accel: Option<String>,
        checked: bool,
        enabled: Option<bool>,
    },
    Separator,
    Submenu {
        label: String,
        items: Vec<CtxItem>,
        enabled: Option<bool>,
    },
}

/// The most items one menu may hold, submenus included: more than the
/// Player's longest menu (three tracks, every speed, several audio tracks)
/// and far less than anything that could slow the system's menu down.
const CTX_MAX_ITEMS: usize = 100;
/// A label's longest: a word picked from a subtitle is cut by the page well before this.
const CTX_MAX_LABEL: usize = 80;
/// The longest command, "speed:1.25" style.
const CTX_MAX_COMMAND: usize = 64;

/// A Player command a menu item may send: a name, and at most one short
/// argument after a colon (`speed:1.5`, `audio:2`). Nothing else ever goes
/// back to the page.
pub fn context_command(command: &str) -> Option<&str> {
    if command.is_empty() || command.len() > CTX_MAX_COMMAND {
        return None;
    }
    let (name, arg) = match command.split_once(':') {
        Some((n, a)) => (n, Some(a)),
        None => (command, None),
    };
    let name_ok = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric());
    let arg_ok = arg.is_none_or(|a| {
        (1..=32).contains(&a.len())
            && a.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    });
    (name_ok && arg_ok).then_some(command)
}

/// Checks a menu the page sent: at most two levels, `CTX_MAX_ITEMS` items,
/// short non-empty labels and well-formed commands.
pub fn check_context_menu(items: &[CtxItem]) -> Result<(), &'static str> {
    fn walk(items: &[CtxItem], depth: usize, count: &mut usize) -> Result<(), &'static str> {
        for item in items {
            *count += 1;
            if *count > CTX_MAX_ITEMS {
                return Err("too many items");
            }
            match item {
                CtxItem::Item { command, label, .. } | CtxItem::Check { command, label, .. } => {
                    if context_command(command).is_none() {
                        return Err("not a command");
                    }
                    if label.trim().is_empty() || label.chars().count() > CTX_MAX_LABEL {
                        return Err("not a label");
                    }
                }
                CtxItem::Separator => {}
                CtxItem::Submenu { label, items, .. } => {
                    if depth >= 1 {
                        return Err("too deep");
                    }
                    if label.trim().is_empty() || label.chars().count() > CTX_MAX_LABEL {
                        return Err("not a label");
                    }
                    walk(items, depth + 1, count)?;
                }
            }
        }
        Ok(())
    }
    let mut count = 0;
    walk(items, 0, &mut count)
}

fn context_items<R: tauri::Runtime>(
    app: &AppHandle<R>,
    items: &[CtxItem],
) -> tauri::Result<Vec<Box<dyn tauri::menu::IsMenuItem<R>>>> {
    use tauri::menu::{CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu};
    let id = |command: &str| format!("{}{command}", crate::app_menu::CONTEXT_PREFIX);
    let mut out: Vec<Box<dyn tauri::menu::IsMenuItem<R>>> = Vec::with_capacity(items.len());
    for item in items {
        match item {
            CtxItem::Item {
                command,
                label,
                accel,
                enabled,
            } => out.push(Box::new(MenuItem::with_id(
                app,
                id(command),
                label,
                enabled.unwrap_or(true),
                accel.as_deref(),
            )?)),
            CtxItem::Check {
                command,
                label,
                accel,
                checked,
                enabled,
            } => out.push(Box::new(CheckMenuItem::with_id(
                app,
                id(command),
                label,
                enabled.unwrap_or(true),
                *checked,
                accel.as_deref(),
            )?)),
            CtxItem::Separator => out.push(Box::new(PredefinedMenuItem::separator(app)?)),
            CtxItem::Submenu {
                label,
                items,
                enabled,
            } => {
                let inner = context_items(app, items)?;
                let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> =
                    inner.iter().map(|b| b.as_ref()).collect();
                out.push(Box::new(Submenu::with_items(
                    app,
                    label,
                    enabled.unwrap_or(true),
                    &refs,
                )?));
            }
        }
    }
    Ok(out)
}

/// The Player's right-click menu (owner, 26 Oct 2026: like IINA's or VLC's):
/// the system's own menu at the pointer, built from what the page sent. A
/// chosen item sends its command to the page as the menu bar does
/// (`player:menu`, through `app_menu::handle`).
#[tauri::command]
pub async fn player_context_menu(
    window: tauri::WebviewWindow,
    items: Vec<CtxItem>,
    position: Option<MenuAt>,
) -> Result<(), String> {
    if window.label() != LABEL {
        return Err("not the Player".into());
    }
    check_context_menu(&items)?;
    let app = window.app_handle().clone();
    let built = context_items(&app, &items).map_err(|e| e.to_string())?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        built.iter().map(|b| b.as_ref()).collect();
    let menu = tauri::menu::Menu::with_items(&app, &refs).map_err(|e| e.to_string())?;
    match position.and_then(MenuAt::checked) {
        // Opened from the keyboard (Shift+F10, the menu key): at the focused thing.
        Some(at) => window
            .popup_menu_at(&menu, tauri::LogicalPosition::new(at.x, at.y))
            .map_err(|e| e.to_string()),
        None => window.popup_menu(&menu).map_err(|e| e.to_string()),
    }
}

/// Where a keyboard-opened menu goes: logical pixels from the window's
/// top-left corner (the focused word or the player).
#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq)]
pub struct MenuAt {
    pub x: f64,
    pub y: f64,
}

/// The farthest a menu may be placed from the window's corner: far beyond
/// any screen, so only nonsense is refused.
const MENU_AT_MAX: f64 = 100_000.0;

impl MenuAt {
    /// A finite place inside a sane range, or none (the pointer is used).
    pub fn checked(self) -> Option<MenuAt> {
        let ok = |v: f64| v.is_finite() && (0.0..=MENU_AT_MAX).contains(&v);
        (ok(self.x) && ok(self.y)).then_some(self)
    }
}

/// The longest text "Copy Line" may put on the clipboard: several subtitle
/// lines at once, far more than one cue ever holds.
const COPY_MAX_CHARS: usize = 4000;

/// Whether a text may go on the clipboard: not empty, not longer than
/// `COPY_MAX_CHARS`, and no control characters but line breaks and tabs.
pub fn copyable(text: &str) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= COPY_MAX_CHARS
        && text
            .chars()
            .all(|c| !c.is_control() || c == '\n' || c == '\t')
}

/// "Copy Line" (the Player's right-click menu): the subtitle line on screen,
/// onto the system clipboard. Only the Player may ask, and only plain text.
#[tauri::command]
pub fn player_copy_text(window: tauri::WebviewWindow, text: String) -> Result<bool, String> {
    if window.label() != LABEL {
        return Err("not the Player".into());
    }
    if !copyable(&text) {
        return Err("not a line".into());
    }
    Ok(crate::capture::write_text(&text))
}

/// What the Player's card needs: signed in or not, the notebooks, and the
/// one words go to. The same reading as the desktop card's `state`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    connected: bool,
    /// The signed-in account's id (null signed out), so the Player labels
    /// what it records with the core's account, never another window's.
    user_id: Option<String>,
    notebooks: Vec<serde_json::Value>,
    notebook_id: Option<String>,
}

#[tauri::command]
pub async fn player_account(app: AppHandle) -> Result<Account, String> {
    let s = crate::commands::state(app.clone(), app.state::<AppState>()).await?;
    Ok(Account {
        connected: s.connected,
        user_id: s.user_id(),
        notebooks: s.notebooks,
        notebook_id: s.notebook_id,
    })
}

/// Full screen: macOS's own space, Windows' borderless screen (§2, §19).
#[tauri::command]
pub fn player_fullscreen(window: WebviewWindow, on: bool) -> Result<(), String> {
    window.set_fullscreen(on).map_err(|e| e.to_string())
}

/// The mini player: small, on top, in the screen's corner (§13).
#[tauri::command]
pub fn player_mini_open(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if window.is_fullscreen().unwrap_or(false) {
        let _ = window.set_fullscreen(false);
    }
    if let Some(big) = placement::window_rect(&window) {
        if let Ok(mut b) = BEFORE_MINI.lock() {
            b.get_or_insert(big);
        }
    }
    let screen = screen_of(&app, &window).ok_or("no screen")?;
    let unit = screen.unit(placement::LOGICAL);
    let _ = window.set_min_size(Some(tauri::LogicalSize::new(MINI_MIN.0, MINI_MIN.1)));
    placement::apply(&window, mini_place(screen.work, unit)).map_err(|e| e.to_string())?;
    window.set_always_on_top(true).map_err(|e| e.to_string())
}

/// Back to the big window, where it was.
#[tauri::command]
pub fn player_back_to_big(window: WebviewWindow) -> Result<(), String> {
    let _ = window.set_always_on_top(false);
    let _ = window.set_min_size(Some(tauri::LogicalSize::new(MIN_SIZE.0, MIN_SIZE.1)));
    let big = BEFORE_MINI.lock().ok().and_then(|mut b| b.take());
    if let Some(rect) = big {
        placement::apply(&window, rect).map_err(|e| e.to_string())?;
    }
    let _ = window.set_focus();
    Ok(())
}

/// The mini player's close button closes the mini window and the Player.
#[tauri::command]
pub fn player_mini_close(window: WebviewWindow) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

/// The mini player's pin: kept on top or not.
#[tauri::command]
pub fn player_on_top(window: WebviewWindow, on: bool) -> Result<(), String> {
    window.set_always_on_top(on).map_err(|e| e.to_string())
}

/// macOS: the traffic lights come and go with the player's chrome (§2).
#[tauri::command]
pub fn player_buttons(app: AppHandle, window: WebviewWindow, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        let win = window.clone();
        let _ = app.run_on_main_thread(move || mac::buttons(&win, visible));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, window, visible);
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    use tauri::WebviewWindow;

    /// Hides or shows the three window buttons (the window's own; nothing else changes).
    pub fn buttons(win: &WebviewWindow, visible: bool) {
        let Ok(ptr) = win.ns_window() else {
            return;
        };
        if ptr.is_null() {
            return;
        }
        // SAFETY: Tauri hands out the window's NSWindow, alive while `win`
        // is; this runs on the main thread (`run_on_main_thread`).
        let ns: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
        for kind in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            if let Some(button) = ns.standardWindowButton(kind) {
                button.setHidden(!visible);
            }
        }
    }
}

#[cfg(windows)]
mod taskbar {
    use tauri::WebviewWindow;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Storage::EnhancedStorage::{
        PKEY_AppUserModel_ID, PKEY_AppUserModel_RelaunchCommand,
        PKEY_AppUserModel_RelaunchDisplayNameResource,
    };
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::UI::Shell::PropertiesSystem::{
        IPropertyStore, SHGetPropertyStoreForWindow,
    };

    /// Gives the Player's window its own taskbar button (§24.3): its own
    /// AppUserModelID, and what a pinned button starts (`Lexpad.exe --player`).
    pub fn own_button(win: &WebviewWindow) {
        let Ok(handle) = win.hwnd() else {
            return;
        };
        let hwnd = HWND(handle.0 as _);
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let relaunch = format!("\"{exe}\" {}", super::PLAYER_FLAG);
        // SAFETY: a property store for our own top-level window; every value
        // is a string PROPVARIANT that outlives the call.
        unsafe {
            let Ok(store) = SHGetPropertyStoreForWindow::<IPropertyStore>(hwnd) else {
                return;
            };
            let id = PROPVARIANT::from(super::APP_USER_MODEL_ID);
            let cmd = PROPVARIANT::from(relaunch.as_str());
            let name = PROPVARIANT::from(super::TITLE);
            let _ = store.SetValue(&PKEY_AppUserModel_ID, &id);
            let _ = store.SetValue(&PKEY_AppUserModel_RelaunchCommand, &cmd);
            let _ = store.SetValue(&PKEY_AppUserModel_RelaunchDisplayNameResource, &name);
            let _ = store.Commit();
        }
    }

    /// DWMWA_COLOR_NONE: no border colour at all (Windows 11).
    const COLOR_NONE: u32 = 0xFFFF_FFFE;

    /// Windows 11 draws a 1-pixel border round a frameless window, in the
    /// accent colour when "Show accent colour on title bars and window
    /// borders" is on: a faint blue line at the foot of the black Player
    /// (owner, 26 Oct 2026), which the Mac never had. The border colour is
    /// set to none; the shadow and the rounded corners stay. Windows 10 has
    /// no such border and ignores the attribute.
    pub fn no_border(win: &WebviewWindow) {
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR};
        let Ok(handle) = win.hwnd() else {
            return;
        };
        let hwnd = HWND(handle.0 as _);
        let color = COLOR_NONE;
        // SAFETY: our own top-level window; the value is a u32 that outlives the call.
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                (&color as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::Screen;

    fn screen(x: f64, y: f64, w: f64, h: f64) -> Screen {
        Screen {
            area: Area::new(x, y, w, h),
            work: Area::new(x, y + 25.0, w, h - 25.0 - 70.0),
            scale: 2.0,
        }
    }

    #[test]
    fn the_first_window_is_72_percent_of_the_work_area_at_16_9_centred() {
        let s = screen(0.0, 0.0, 1440.0, 900.0);
        let r = initial_place(&[s], Some((700.0, 400.0)), None, true).unwrap();
        assert!(
            (r.h - s.work.h * WORK_SHARE).abs() < 0.5 || (r.w - s.work.w * WORK_SHARE).abs() < 0.5
        );
        assert!(((r.w / r.h) - 16.0 / 9.0).abs() < 0.01);
        let (cx, cy) = r.centre();
        let (wx, wy) = s.work.centre();
        assert!((cx - wx).abs() < 0.5 && (cy - wy).abs() < 0.5);
        assert!(s.work.holds(&r));
    }

    #[test]
    fn a_remembered_place_comes_back_inside_its_monitor() {
        let s = screen(0.0, 0.0, 1440.0, 900.0);
        let saved = WindowPlace {
            x: 1200.0,
            y: 700.0,
            w: 800.0,
            h: 450.0,
            maximized: false,
        };
        let r = initial_place(&[s], None, Some(saved), true).unwrap();
        assert!(s.work.holds(&r));
        assert_eq!((r.w, r.h), (800.0, 450.0));
    }

    #[test]
    fn the_mini_player_sits_16_points_in_from_the_corner() {
        let work = Area::new(0.0, 25.0, 1440.0, 805.0);
        let r = mini_place(work, 1.0);
        assert_eq!((r.w, r.h), (448.0, 252.0));
        assert_eq!(r.right(), work.right() - 16.0);
        assert_eq!(r.bottom(), work.bottom() - 16.0);
        let px = mini_place(work, 2.0);
        assert_eq!((px.w, px.h), (896.0, 504.0));
    }

    #[test]
    fn the_route_script_starts_the_router_on_the_player() {
        let s = init_script("dev", Opened::Pick, false, None);
        assert!(s.contains("window.__LEXPAD_CONFIG"));
        assert!(s.contains("#/player?from=pick'"));
        assert!(init_script("dev", Opened::Launch, false, None).ends_with("'#/player');"));
        // After the Start window, the Player is told so (no second sign-in).
        assert!(
            init_script("dev", Opened::File, true, None).contains("#/player?from=file&seen=start'")
        );
        assert!(init_script("dev", Opened::Launch, true, None).ends_with("'#/player?seen=start');"));
    }

    #[test]
    fn the_route_says_which_door_opened_the_player() {
        assert!(init_script("dev", Opened::Pick, false, Some(Door::Tray))
            .contains("#/player?from=pick&door=tray'"));
        assert!(
            init_script("dev", Opened::Launch, false, Some(Door::Launch))
                .ends_with("'#/player?door=launch');")
        );
        assert!(init_script("dev", Opened::File, true, Some(Door::Drop))
            .contains("#/player?from=file&seen=start&door=drop'"));
    }

    #[test]
    fn every_door_has_one_name_and_reads_back() {
        let names: Vec<&str> = Door::ALL.iter().map(|d| d.as_str()).collect();
        assert_eq!(
            names,
            [
                "start",
                "launch",
                "tray",
                "watch",
                "today_card",
                "first_five",
                "popup_hint",
                "drop",
                "file",
                "menu"
            ]
        );
        for d in Door::ALL {
            assert_eq!(Door::parse(d.as_str()), Some(d));
            // The server keeps a detail of [a-z0-9_.-]{1,32}.
            assert!(d
                .as_str()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '_'));
        }
        assert_eq!(Door::parse("Tray"), None);
        assert_eq!(Door::parse(""), None);
    }

    #[test]
    fn a_page_names_only_its_own_doors() {
        assert_eq!(page_door(None), Ok(None));
        assert_eq!(page_door(Some("watch")), Ok(Some(Door::Watch)));
        assert_eq!(page_door(Some("today_card")), Ok(Some(Door::TodayCard)));
        assert_eq!(page_door(Some("first_five")), Ok(Some(Door::FirstFive)));
        assert_eq!(page_door(Some("menu")), Ok(Some(Door::Menu)));
        // The core's doors, and anything else, are refused.
        for f in [
            "tray",
            "file",
            "drop",
            "launch",
            "start",
            "popup_hint",
            "x",
            "watch&x=1",
        ] {
            assert!(page_door(Some(f)).is_err(), "{f}");
        }
    }

    #[test]
    fn a_file_id_is_never_a_path() {
        assert!(is_file_id("a1-B_2"));
        assert!(!is_file_id(""));
        assert!(!is_file_id("../x"));
        assert!(!is_file_id(&"a".repeat(65)));
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;

    fn item(command: &str, label: &str) -> CtxItem {
        CtxItem::Item {
            command: command.into(),
            label: label.into(),
            accel: None,
            enabled: None,
        }
    }

    #[test]
    fn only_plain_commands_go_back_to_the_page() {
        for ok in [
            "playPause",
            "speed:1.25",
            "audio:2",
            "toggleTrack1",
            "addWord",
        ] {
            assert_eq!(context_command(ok), Some(ok), "{ok}");
        }
        for bad in [
            "",
            "1play",
            "play pause",
            "speed:",
            "speed:1:2",
            "open:../x",
            "speed:<b>",
            "a:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert_eq!(context_command(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_page_describes_the_menu_in_json() {
        let items: Vec<CtxItem> = serde_json::from_str(
            r#"[{"kind":"item","command":"playPause","label":"Pause","accel":"Space"},
                {"kind":"separator"},
                {"kind":"submenu","label":"Speed","items":[
                  {"kind":"check","command":"speed:1","label":"Normal","checked":true}]}]"#,
        )
        .unwrap();
        assert_eq!(items.len(), 3);
        assert!(check_context_menu(&items).is_ok());
    }

    #[test]
    fn only_plain_lines_go_on_the_clipboard() {
        assert!(copyable("Look Celia, we have to follow our passions;"));
        assert!(copyable("two\nlines"));
        assert!(!copyable("   "));
        assert!(!copyable("bell\u{7}"));
        assert!(!copyable(&"x".repeat(COPY_MAX_CHARS + 1)));
    }

    #[test]
    fn a_keyboard_menu_goes_where_the_page_says_within_reason() {
        let at = MenuAt { x: 120.0, y: 48.5 };
        assert_eq!(at.checked(), Some(at));
        assert_eq!(MenuAt { x: -1.0, y: 3.0 }.checked(), None);
        assert_eq!(
            MenuAt {
                x: f64::NAN,
                y: 3.0
            }
            .checked(),
            None
        );
        assert_eq!(
            MenuAt {
                x: 3.0,
                y: f64::INFINITY
            }
            .checked(),
            None
        );
    }

    #[test]
    fn a_menu_too_deep_too_long_or_badly_labelled_is_refused() {
        let deep = vec![CtxItem::Submenu {
            label: "A".into(),
            enabled: None,
            items: vec![CtxItem::Submenu {
                label: "B".into(),
                enabled: None,
                items: vec![item("playPause", "x")],
            }],
        }];
        assert_eq!(check_context_menu(&deep), Err("too deep"));
        let long: Vec<CtxItem> = (0..=CTX_MAX_ITEMS)
            .map(|_| item("playPause", "x"))
            .collect();
        assert_eq!(check_context_menu(&long), Err("too many items"));
        assert_eq!(
            check_context_menu(&[item("playPause", " ")]),
            Err("not a label")
        );
        let wide = "w".repeat(CTX_MAX_LABEL + 1);
        assert_eq!(
            check_context_menu(&[item("playPause", &wide)]),
            Err("not a label")
        );
        assert_eq!(
            check_context_menu(&[item("rm -rf", "x")]),
            Err("not a command")
        );
    }
}

#[cfg(test)]
mod close_tests {
    use super::{CloseGuard, CloseStep};

    #[test]
    fn a_first_close_waits_a_second_or_the_timeout_closes() {
        let mut g = CloseGuard::new();
        // First request: armed; the page has its moment.
        let CloseStep::Arm(t1) = g.request() else {
            panic!("first request must arm")
        };
        // A second request (the learner pressed again): closed at once.
        assert_eq!(g.request(), CloseStep::Destroy);
        // The first timer then finds nothing to do.
        assert!(!g.timed_out(t1));

        // A first request whose page never answers: the timer closes it.
        let CloseStep::Arm(t2) = g.request() else {
            panic!()
        };
        assert!(g.timed_out(t2));
        assert!(!g.timed_out(t2), "only once");

        // The page closed it in time: the timer is a no-op.
        let CloseStep::Arm(t3) = g.request() else {
            panic!()
        };
        g.closed();
        assert!(!g.timed_out(t3));

        // A timer from an earlier window never closes a newer one.
        let CloseStep::Arm(t4) = g.request() else {
            panic!()
        };
        g.closed();
        let CloseStep::Arm(t5) = g.request() else {
            panic!()
        };
        assert!(!g.timed_out(t4));
        assert!(g.timed_out(t5));
    }

    #[test]
    fn the_player_windows_may_destroy_themselves() {
        // The page's close listener ends with destroy(); without the
        // permission the window could never close.
        for (name, file) in [
            ("player.json", include_str!("../capabilities/player.json")),
            (
                "player-dev.json",
                include_str!("../capabilities/player-dev.json"),
            ),
        ] {
            let v: serde_json::Value = serde_json::from_str(file).unwrap();
            let perms: Vec<&str> = v["permissions"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|p| p.as_str())
                .collect();
            assert!(perms.contains(&"core:window:allow-destroy"), "{name}");
        }
    }
}

#[cfg(test)]
mod account_tests {
    #[test]
    fn the_player_is_told_the_cores_account_or_null() {
        let signed_in = super::Account {
            connected: true,
            user_id: Some("u1".into()),
            notebooks: vec![],
            notebook_id: None,
        };
        let v = serde_json::to_value(&signed_in).unwrap();
        assert_eq!(v["userId"], "u1");
        let out = super::Account {
            connected: false,
            user_id: None,
            notebooks: vec![],
            notebook_id: None,
        };
        assert!(serde_json::to_value(&out).unwrap()["userId"].is_null());
    }
}

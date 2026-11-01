//! Whether the Services menu's "Add to Lexpad" is live yet, for the Mac App
//! Store build, where ⌘⇧L is that service's key (Info.mas.plist).
//!
//! On a first launch after an install (TestFlight, the App Store) macOS may
//! not have read the new app's services yet when it starts, and other apps
//! then do not know the key until the services list is rebuilt: the owner's
//! ⌘⇧L did nothing until Lexpad was opened a second time (1 Nov 2026). So the
//! core asks for its own service with a private probe (`NSPerformService` on
//! a pasteboard of its own, carrying a token only this run knows), and once
//! the probe arrives it asks macOS to rebuild the list again
//! (`NSUpdateDynamicServices`), which is what the second launch did. Until
//! then the panel says the key is being set up; after it, "Press ⌘⇧L in any
//! app". The probe never opens a card.
//!
//! The probe is not sent while another Lexpad (the copy from lexpad.app,
//! `app.lexpad.desktop`) is running: its service has the same name and would
//! receive it, and its global shortcut takes ⌘⇧L first. The panel says so.
//!
//! The rules are pure here (`ServiceState`, `next_step`), tested; the AppKit calls
//! are in services_macos.rs.

use std::time::Duration;

/// How often the probe is sent until the service answers.
pub const PROBE_EVERY: Duration = Duration::from_millis(1500);
/// How many probes, then nothing more this run (the list is rebuilt every
/// `REFRESH_EVERY` probes meanwhile). 80 × 1.5 s is two minutes.
pub const PROBE_MAX: u32 = 80;
/// While the probe is not answered, ask macOS to rebuild the list again every
/// this many probes (the install may still be settling). On a fresh install
/// (scripts/mas-firstrun-test.sh, 1 Nov 2026) the list built as the app
/// started did not have it yet, and the first rebuild after it did.
pub const REFRESH_EVERY: u32 = 3;

/// Where the service stands this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    /// Not answered yet; probing.
    Pending,
    /// The probe arrived: the service and its key are live.
    Live,
    /// Another Lexpad is running and would take the probe and the key.
    OtherCopy,
    /// No answer after `PROBE_MAX` probes.
    NotFound,
}

/// What the probe loop does next.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Send a probe (and rebuild the list first when `refresh`).
    Probe { refresh: bool },
    /// The service answered: rebuild the list once more and stop.
    Done,
    /// Wait without probing (another copy is running).
    Wait,
    /// Give up for this run.
    GiveUp,
}

/// The next step after `tries` probes.
pub fn next_step(tries: u32, answered: bool, other_copy: bool) -> Step {
    if answered {
        return Step::Done;
    }
    if tries >= PROBE_MAX {
        return Step::GiveUp;
    }
    if other_copy {
        return Step::Wait;
    }
    Step::Probe {
        refresh: tries > 0 && tries % REFRESH_EVERY == 0,
    }
}

/// The state the loop reports for a step.
pub fn live_for(step: &Step, other_copy: bool) -> ServiceState {
    match step {
        Step::Done => ServiceState::Live,
        Step::GiveUp if other_copy => ServiceState::OtherCopy,
        Step::GiveUp => ServiceState::NotFound,
        Step::Wait => ServiceState::OtherCopy,
        Step::Probe { .. } => ServiceState::Pending,
    }
}

/// Whether a service call is this run's probe (and so opens nothing).
pub fn is_probe(text: Option<&str>, token: &str) -> bool {
    !token.is_empty() && text.map(str::trim) == Some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_until_the_service_answers_then_rebuilds_once_more() {
        assert_eq!(next_step(0, false, false), Step::Probe { refresh: false });
        assert_eq!(next_step(1, false, false), Step::Probe { refresh: false });
        assert_eq!(
            next_step(REFRESH_EVERY, false, false),
            Step::Probe { refresh: true }
        );
        assert_eq!(next_step(3, true, false), Step::Done);
        assert_eq!(live_for(&Step::Done, false), ServiceState::Live);
        assert_eq!(
            live_for(&Step::Probe { refresh: false }, false),
            ServiceState::Pending
        );
    }

    #[test]
    fn never_probes_while_the_other_copy_runs() {
        assert_eq!(next_step(2, false, true), Step::Wait);
        assert_eq!(live_for(&Step::Wait, true), ServiceState::OtherCopy);
        // Its answer still counts: the other copy quit and ours answered.
        assert_eq!(next_step(2, true, true), Step::Done);
    }

    #[test]
    fn gives_up_after_two_minutes() {
        assert_eq!(next_step(PROBE_MAX, false, false), Step::GiveUp);
        assert_eq!(live_for(&Step::GiveUp, false), ServiceState::NotFound);
        assert_eq!(live_for(&Step::GiveUp, true), ServiceState::OtherCopy);
        assert_eq!(PROBE_EVERY * PROBE_MAX, Duration::from_secs(120));
    }

    #[test]
    fn only_this_runs_token_is_a_probe() {
        assert!(is_probe(Some("lexpad-probe-abc"), "lexpad-probe-abc"));
        assert!(is_probe(Some(" lexpad-probe-abc\n"), "lexpad-probe-abc"));
        assert!(!is_probe(Some("serendipity"), "lexpad-probe-abc"));
        assert!(!is_probe(None, "lexpad-probe-abc"));
        assert!(!is_probe(Some(""), ""));
    }
}

//! Words saved before they can reach the account: while signed out (the
//! player needs no account; signing in is asked for at the first save), or
//! offline. They wait in `pending-words.json` in the app's data folder, on
//! this computer only, and go to the account when it is there: right after
//! signing in (`connect`), at start when signed in, and when a window asks
//! (`words_flush`, e.g. when it comes back online).
//!
//! The core had no such queue before (the popup's `add_word` needs a
//! session; Lexpad's window keeps its own outbox in IndexedDB, but only for
//! a signed-in account), so this is it, for every window that may save
//! without a session.
//!
//! Each word goes to the notebook for its language (`targetLang`): the
//! chosen notebook (Settings) when it is in that language, else the
//! account's default one in it, else the first one in it. A word with no
//! notebook in its language stays queued with that reason, for the window
//! to offer making one; nothing is ever put in a notebook of another
//! language. A word queued while signed in to one account is never sent to
//! another.
//!
//! A word saved signed out goes to the account that signs in only while it
//! is at most `ANONYMOUS_MAX_AGE_MS` old (owner, 26 Oct 2026: the same 24 h
//! as anonymous activity events, spec §10). An older one is never moved to
//! an account by itself: it stays on this computer, unsent, marked
//! `too_old_to_move`, for the learner to see and discard (or save again,
//! which is their own choice). Kept rather than dropped because it is the
//! learner's own word; nothing about it leaves this computer. A word whose
//! time is ahead of the clock by more than `CLOCK_SKEW_MS` (the clock was put
//! back) has an age nobody can tell, and is kept the same way.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::{Api, Failure, Surface};

/// How old a word saved signed out may be and still go to the account that signs in.
pub const ANONYMOUS_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;
/// How far ahead of this clock a saved time may be (a clock corrected by a
/// few minutes) and still count as just saved.
pub const CLOCK_SKEW_MS: u64 = 5 * 60 * 1000;
/// The reason a too-old (or undatable) signed-out word waits with.
pub const TOO_OLD: &str = "too_old_to_move";

/// Whether a queued word may go to the account `user` now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// Its own account, or saved signed out within the last 24 hours.
    Send,
    /// Saved signed out longer ago (or at a time ahead of the clock): kept here, unsent.
    TooOld,
    /// Another account's: never sent under this one.
    NotThis,
}

/// The rule for one word (see the module's head).
pub fn may_join(p: &Pending, user: &str, now: u64) -> Join {
    match p.account.as_deref() {
        Some(a) if a == user => Join::Send,
        Some(_) => Join::NotThis,
        None if p.saved_at > now.saturating_add(CLOCK_SKEW_MS) => Join::TooOld,
        None if now.saturating_sub(p.saved_at) <= ANONYMOUS_MAX_AGE_MS => Join::Send,
        None => Join::TooOld,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// The most words kept waiting; a save beyond it is refused, never dropped
/// silently.
const MAX_PENDING: usize = 500;
/// The API's limits, checked before a word is queued so it cannot fail later.
const MAX_HEADWORD: usize = 200;
const MAX_SENTENCE: usize = 1000;
const MAX_MEMO: usize = 500;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub id: String,
    pub headword: String,
    /// ISO 639-1: the language of the subtitle it came from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// The line it was met in, kept as its first example.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sentence: Option<String>,
    /// The learner's private note, as the window composed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    pub saved_at: u64,
    /// The account signed in when it was saved; `None` (signed out) goes to
    /// whoever signs in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Why it is still waiting, after the last try.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
    /// Where in the player it was picked, for the activity reports; absent
    /// in a file from before, which reads as the subtitles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<PickedIn>,
}

/// Where in the player a word was picked. A fixed name, never anything about the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PickedIn {
    /// The subtitles on the video.
    Subtitles,
    /// The transcript panel.
    Transcript,
}

impl PickedIn {
    /// The activity surface a word picked here is sent with.
    pub fn surface(self) -> Surface {
        match self {
            PickedIn::Subtitles => Surface::Player,
            PickedIn::Transcript => Surface::PlayerTranscript,
        }
    }
}

/// What a window asks to save.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewWord {
    pub headword: String,
    pub lang: Option<String>,
    pub sentence: Option<String>,
    pub memo: Option<String>,
    /// Where in the player it was picked; anything but the two names is refused.
    #[serde(default)]
    pub from: Option<PickedIn>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "status"
)]
pub enum Saved {
    /// In the notebook now.
    Added { id: String, notebook_id: String },
    /// Waiting on this computer, and why.
    Queued { pending: usize, reason: String },
}

fn clean(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect::<String>()
        .trim()
        .chars()
        .take(max)
        .collect()
}

impl NewWord {
    /// Checked and trimmed, or why not.
    pub fn checked(self, account: Option<String>, now: u64) -> Result<Pending, String> {
        let headword = clean(&self.headword.replace('\n', " "), MAX_HEADWORD + 1);
        if headword.is_empty() || headword.chars().count() > MAX_HEADWORD {
            return Err("headword".into());
        }
        let lang = self
            .lang
            .map(|l| l.trim().to_ascii_lowercase())
            .filter(|l| (2..=3).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_lowercase()));
        let memo = self
            .memo
            .map(|m| clean(&m, MAX_MEMO + 1))
            .filter(|m| !m.is_empty());
        if memo.as_ref().is_some_and(|m| m.chars().count() > MAX_MEMO) {
            return Err("memo".into());
        }
        Ok(Pending {
            id: uuid::Uuid::new_v4().simple().to_string(),
            headword,
            lang,
            sentence: self
                .sentence
                .map(|s| clean(&s.replace('\n', " "), MAX_SENTENCE))
                .filter(|s| !s.is_empty()),
            memo,
            saved_at: now,
            account,
            waiting: None,
            from: self.from,
        })
    }
}

impl Pending {
    /// The activity surface it is sent with (`X-Lexpad-Surface`).
    pub fn surface(&self) -> Surface {
        self.from.unwrap_or(PickedIn::Subtitles).surface()
    }

    /// The API's WordCreate for it: a word the learner chose, the line it
    /// was in as its example; the AI completes it later in the app, on the
    /// learner's request, as any word added by hand.
    pub fn body(&self) -> Value {
        let mut w = json!({ "headword": self.headword, "category": "general", "source": "manual" });
        if let Some(s) = &self.sentence {
            w["meanings"] = json!([{ "meaning": "—", "examples": [{ "sentence": s }] }]);
        }
        if let Some(m) = &self.memo {
            w["memo"] = json!(m);
        }
        w
    }
}

/// The notebook a word in `lang` goes to (see the module's doc).
pub fn pick_notebook(
    notebooks: &[Value],
    lang: Option<&str>,
    chosen: Option<&str>,
) -> Option<String> {
    let id = |n: &Value| n.get("id").and_then(Value::as_str).map(str::to_owned);
    let target = |n: &Value| {
        n.get("targetLang")
            .and_then(Value::as_str)
            .map(|l| l.to_ascii_lowercase())
    };
    let default = |n: &Value| n.get("isDefault").and_then(Value::as_bool).unwrap_or(false);
    let fits = |n: &Value| match lang {
        Some(l) => target(n).as_deref() == Some(l),
        None => true,
    };
    if let Some(c) = chosen {
        if let Some(n) = notebooks
            .iter()
            .find(|n| id(n).as_deref() == Some(c) && fits(n))
        {
            return id(n);
        }
    }
    notebooks
        .iter()
        .find(|n| fits(n) && default(n))
        .or_else(|| notebooks.iter().find(|n| fits(n)))
        .and_then(id)
}

pub struct WordQueue {
    file: Option<PathBuf>,
    items: Mutex<Vec<Pending>>,
    /// One flush at a time, so a word is never sent twice.
    flushing: tokio::sync::Mutex<()>,
}

impl WordQueue {
    pub fn load(file: Option<PathBuf>) -> WordQueue {
        let items = file
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        WordQueue {
            file,
            items: Mutex::new(items),
            flushing: tokio::sync::Mutex::new(()),
        }
    }

    fn save(&self, items: &[Pending]) -> Result<(), String> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(items).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, file).map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Vec<Pending> {
        self.items.lock().map(|i| i.clone()).unwrap_or_default()
    }

    pub fn push(&self, p: Pending) -> Result<usize, String> {
        let mut items = self.items.lock().map_err(|_| "lock")?;
        if items.len() >= MAX_PENDING {
            return Err("queue_full".into());
        }
        items.push(p);
        self.save(&items)?;
        Ok(items.len())
    }

    pub fn discard(&self, id: &str) -> Result<usize, String> {
        let mut items = self.items.lock().map_err(|_| "lock")?;
        items.retain(|p| p.id != id);
        self.save(&items)?;
        Ok(items.len())
    }

    fn set_waiting(&self, id: &str, why: &str) {
        if let Ok(mut items) = self.items.lock() {
            if let Some(p) = items.iter_mut().find(|p| p.id == id) {
                p.waiting = Some(why.to_owned());
            }
            let _ = self.save(&items);
        }
    }

    /// Sends one waiting word now, on the learner's tap: to the signed-in
    /// account, whatever its age (the 24-hour rule is about moving words by
    /// themselves; a tap is the learner choosing), with its own surface.
    /// Refused for a word another account saved, and for an id not here.
    /// What could not go stays, with the reason, as `words_save` answers.
    pub async fn send_one(
        &self,
        api: &Api,
        id: &str,
        chosen: Option<&str>,
    ) -> Result<Saved, String> {
        let _one = self.flushing.lock().await;
        let p = self
            .list()
            .into_iter()
            .find(|p| p.id == id)
            .ok_or("not_found")?;
        // A word kept as too old stays marked so, whatever this try met: the
        // learner sees it in the same list, and the answer says why it did
        // not go this time.
        let queued = |reason: &str| -> Result<Saved, String> {
            if p.waiting.as_deref() != Some(TOO_OLD) {
                self.set_waiting(&p.id, reason);
            }
            Ok(Saved::Queued {
                pending: self.list().len(),
                reason: reason.to_owned(),
            })
        };
        let Some(user) = api.user().await else {
            return queued(Failure::SignedOut.as_str());
        };
        if p.account.as_deref().is_some_and(|a| a != user.id) {
            return Err("not_this_account".into());
        }
        let notebooks = match api.notebooks().await {
            Ok(n) => n,
            Err(f) => return queued(f.as_str()),
        };
        let Some(nb) = pick_notebook(&notebooks, p.lang.as_deref(), chosen) else {
            return queued("no_notebook_for_language");
        };
        match api.add_word(&nb, &p.body(), Some(p.surface())).await {
            Ok(word_id) => {
                self.discard(&p.id)?;
                Ok(Saved::Added {
                    id: word_id,
                    notebook_id: nb,
                })
            }
            // Already in the notebook: nothing left to send, and the card says so.
            Err(Failure::Duplicate) => {
                self.discard(&p.id)?;
                Err(Failure::Duplicate.as_str().into())
            }
            Err(f) => queued(f.as_str()),
        }
    }

    /// Sends what can go now. Returns how many went and how many wait.
    pub async fn flush(&self, api: &Api, chosen: Option<&str>) -> (usize, usize) {
        self.flush_at(api, chosen, now_ms()).await
    }

    /// `flush` with the clock given, for tests.
    pub async fn flush_at(&self, api: &Api, chosen: Option<&str>, now: u64) -> (usize, usize) {
        let _one = self.flushing.lock().await;
        let Some(user) = api.user().await else {
            return (0, self.list().len());
        };
        let mut mine: Vec<Pending> = Vec::new();
        for p in self.list() {
            match may_join(&p, &user.id, now) {
                Join::Send => mine.push(p),
                Join::TooOld => {
                    if p.waiting.as_deref() != Some(TOO_OLD) {
                        self.set_waiting(&p.id, TOO_OLD);
                    }
                }
                Join::NotThis => {}
            }
        }
        if mine.is_empty() {
            return (0, self.list().len());
        }
        let notebooks = match api.notebooks().await {
            Ok(n) => n,
            Err(_) => return (0, self.list().len()),
        };
        let mut sent = 0;
        for p in mine {
            let Some(nb) = pick_notebook(&notebooks, p.lang.as_deref(), chosen) else {
                self.set_waiting(&p.id, "no_notebook_for_language");
                continue;
            };
            match api.add_word(&nb, &p.body(), Some(p.surface())).await {
                // Already in the notebook counts as saved.
                Ok(_) | Err(Failure::Duplicate) => {
                    let _ = self.discard(&p.id);
                    sent += 1;
                }
                Err(f @ (Failure::Offline | Failure::SignedOut)) => {
                    self.set_waiting(&p.id, f.as_str());
                    break;
                }
                Err(f) => self.set_waiting(&p.id, f.as_str()),
            }
        }
        (sent, self.list().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nb(id: &str, lang: &str, default: bool) -> Value {
        json!({ "id": id, "title": id, "targetLang": lang, "meaningLang": "en", "isDefault": default })
    }

    fn anonymous(at: u64) -> Pending {
        serde_json::from_value(json!({ "id": "x", "headword": "a", "savedAt": at })).unwrap()
    }

    #[test]
    fn a_word_saved_signed_out_joins_an_account_only_within_24_hours() {
        let now = 5 * ANONYMOUS_MAX_AGE_MS;
        let minute = 60_000;
        assert_eq!(
            may_join(&anonymous(now - (ANONYMOUS_MAX_AGE_MS - minute)), "u", now),
            Join::Send
        );
        assert_eq!(
            may_join(&anonymous(now - ANONYMOUS_MAX_AGE_MS), "u", now),
            Join::Send
        );
        assert_eq!(
            may_join(&anonymous(now - (ANONYMOUS_MAX_AGE_MS + minute)), "u", now),
            Join::TooOld
        );
        // A clock put back a little: still just saved. Put back more: nobody can tell, kept here.
        assert_eq!(
            may_join(&anonymous(now + CLOCK_SKEW_MS), "u", now),
            Join::Send
        );
        assert_eq!(
            may_join(&anonymous(now + CLOCK_SKEW_MS + 1), "u", now),
            Join::TooOld
        );
        // A clock put forward past a day: too old, kept here.
        assert_eq!(
            may_join(&anonymous(now), "u", now + 2 * ANONYMOUS_MAX_AGE_MS),
            Join::TooOld
        );
        // An account's own word goes whatever its age; another account's never.
        let mut own = anonymous(0);
        own.account = Some("u".into());
        assert_eq!(may_join(&own, "u", now), Join::Send);
        assert_eq!(may_join(&own, "v", now), Join::NotThis);
    }

    #[test]
    fn a_word_goes_only_to_a_notebook_in_its_language() {
        let list = [
            nb("A", "en", true),
            nb("B", "fa", false),
            nb("C", "fa", true),
        ];
        assert_eq!(pick_notebook(&list, Some("fa"), None).as_deref(), Some("C"));
        assert_eq!(
            pick_notebook(&list, Some("fa"), Some("B")).as_deref(),
            Some("B")
        );
        // The chosen notebook is in another language: not there.
        assert_eq!(
            pick_notebook(&list, Some("fa"), Some("A")).as_deref(),
            Some("C")
        );
        assert_eq!(pick_notebook(&list, Some("de"), Some("A")), None);
        assert_eq!(pick_notebook(&list, None, Some("B")).as_deref(), Some("B"));
        assert_eq!(pick_notebook(&list, None, None).as_deref(), Some("A"));
    }

    #[test]
    fn words_are_checked_kept_on_disk_and_discarded() {
        assert!(NewWord {
            headword: "  ".into(),
            lang: None,
            sentence: None,
            memo: None,
            from: None,
        }
        .checked(None, 1)
        .is_err());
        let p = NewWord {
            headword: " kitchen\u{7} ".into(),
            lang: Some("EN".into()),
            sentence: Some("I left my keys on the\nkitchen table.".into()),
            memo: Some("Seen in a video".into()),
            from: Some(PickedIn::Transcript),
        }
        .checked(None, 1)
        .unwrap();
        assert_eq!(p.surface(), Surface::PlayerTranscript);
        let old: Pending =
            serde_json::from_str(r#"{"id":"x","headword":"a","savedAt":1}"#).unwrap();
        assert_eq!(old.surface(), Surface::Player);
        assert!(serde_json::from_str::<NewWord>(r#"{"headword":"a","from":"file.mkv"}"#).is_err());
        assert_eq!(
            (p.headword.as_str(), p.lang.as_deref()),
            ("kitchen", Some("en"))
        );
        assert_eq!(
            p.body()["meanings"][0]["examples"][0]["sentence"],
            "I left my keys on the kitchen table."
        );
        let file = std::env::temp_dir().join(format!("lexpad-words-{}.json", uuid::Uuid::new_v4()));
        let q = WordQueue::load(Some(file.clone()));
        q.push(p.clone()).unwrap();
        let again = WordQueue::load(Some(file.clone()));
        assert_eq!(again.list(), vec![p.clone()]);
        assert_eq!(again.discard(&p.id).unwrap(), 0);
        let _ = std::fs::remove_file(file);
    }
}

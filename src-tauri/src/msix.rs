//! Windows: the copy from the Microsoft Store runs from an MSIX package
//! (`msix/AppxManifest.xml`, made by `scripts/msix.ps1` in CI). Two things
//! work differently there, and only there:
//!
//! - **Start on login** is the package's `StartupTask` (`windows.startupTask`
//!   in the manifest, id [`STARTUP_TASK`]), not the `Run` key in the
//!   registry: a packaged app's registry writes are kept inside its package,
//!   so Windows would never see the key. The learner sees the task, and can
//!   turn it off, under Settings → Apps → Startup and in Task Manager; once
//!   they turn it off there, only they can turn it back on.
//! - **Notifications** are filed under the package's own AppUserModelID,
//!   which Windows gives the app, instead of the installer shortcut's.
//!
//! Signing in needs nothing here: the package is a full-trust desktop app
//! (`runFullTrust`), not an AppContainer, so the loopback listener of
//! `auth.rs` takes the browser's redirect exactly as the installed copy does.

use std::sync::OnceLock;

use windows::core::HSTRING;
use windows::ApplicationModel::{StartupTask, StartupTaskState};
use windows::Win32::Foundation::APPMODEL_ERROR_NO_PACKAGE;
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// The `TaskId` of the StartupTask in `msix/AppxManifest.xml`.
pub const STARTUP_TASK: &str = "LexpadStartup";

/// Whether this process runs with a package identity (the Store's MSIX).
pub fn is_packaged() -> bool {
    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(|| {
        let mut len = 0u32;
        // With no buffer this only asks for the name's length: it fails with
        // "no package" outside a package and "buffer too small" inside one.
        let rc = unsafe { GetCurrentPackageFullName(&mut len, None) };
        rc != APPMODEL_ERROR_NO_PACKAGE
    })
}

fn is_on(state: StartupTaskState) -> bool {
    state == StartupTaskState::Enabled || state == StartupTaskState::EnabledByPolicy
}

/// Runs `f` on a thread of its own that has joined COM's multithreaded
/// apartment, so waiting on a WinRT call never blocks a window's thread.
fn on_mta<T: Send + 'static>(
    f: impl FnOnce() -> windows::core::Result<T> + Send + 'static,
) -> Result<T, String> {
    std::thread::spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        f()
    })
    .join()
    .map_err(|_| "the startup task thread panicked".to_string())?
    .map_err(|e| e.to_string())
}

fn task() -> windows::core::Result<StartupTask> {
    StartupTask::GetAsync(&HSTRING::from(STARTUP_TASK))?.get()
}

/// Whether the package's startup task is on.
pub fn startup_enabled() -> Result<bool, String> {
    on_mta(|| Ok(is_on(task()?.State()?)))
}

/// Turns the package's startup task on or off and says whether it is on
/// now. A desktop app's request is granted without a prompt, unless the
/// learner (or a policy) turned the task off in Windows: then it stays off.
pub fn set_startup(on: bool) -> Result<bool, String> {
    on_mta(move || {
        let task = task()?;
        if on {
            Ok(is_on(task.RequestEnableAsync()?.get()?))
        } else {
            task.Disable()?;
            Ok(is_on(task.State()?))
        }
    })
}

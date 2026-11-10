//! "Quit Lexpad" in the taskbar button's right-click menu on Windows (a Jump
//! List task, Windows 10 and 11). Owner, 10 Nov 2026: no Quit inside Lexpad's
//! windows; the tray icon's menu and this one are the ways out.
//!
//! The task starts `Lexpad.exe --quit`. The single-instance plugin hands
//! those arguments to the running Lexpad, which quits the same way the tray's
//! "Quit Lexpad" does (`crate::quit`); the second process ends at once. With
//! no Lexpad running, `--quit` ends before anything opens.
//!
//! The list is written for both taskbar identities: Lexpad's own (the
//! process's, or the MSIX package's) and the Player's (`Lexpad.Player`,
//! player_window.rs), since each has its own taskbar button.

/// The argument the task starts Lexpad with.
pub const QUIT_FLAG: &str = "--quit";
/// The task's name in the menu, the same words as the tray's item.
#[cfg_attr(not(windows), allow(dead_code))]
pub const QUIT_TITLE: &str = "Quit Lexpad";

/// Whether these command-line arguments ask Lexpad to quit.
pub fn wants_quit<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|a| a.as_ref() == QUIT_FLAG)
}

/// Writes the task list for both taskbar buttons, off the main thread (COM
/// and the shell may take a moment). A failure is logged; the tray still quits.
#[cfg(windows)]
pub fn install() {
    std::thread::spawn(|| {
        for id in [None, Some(crate::player_window::APP_USER_MODEL_ID)] {
            // SAFETY: COM objects made and used on this thread only.
            if let Err(e) = unsafe { write_list(id) } {
                log::warn!("jump list for {}: {e}", id.unwrap_or("Lexpad"));
            } else {
                log::info!("jump list for {}: Quit Lexpad", id.unwrap_or("Lexpad"));
            }
        }
    });
}

#[cfg(windows)]
unsafe fn write_list(app_id: Option<&str>) -> windows::core::Result<()> {
    use windows::core::{Interface as _, HSTRING};
    use windows::Win32::Foundation::E_FAIL;
    use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{
        DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
    };

    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    let exe = std::env::current_exe().map_err(|_| windows::core::Error::from(E_FAIL))?;
    let exe = HSTRING::from(exe.as_os_str());

    let list: ICustomDestinationList =
        CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
    if let Some(id) = app_id {
        list.SetAppID(&HSTRING::from(id))?;
    }
    let mut slots = 0u32;
    let _removed: IObjectArray = list.BeginList(&mut slots)?;

    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
    link.SetPath(&exe)?;
    link.SetArguments(&HSTRING::from(QUIT_FLAG))?;
    link.SetIconLocation(&exe, 0)?;
    link.SetDescription(&HSTRING::from(QUIT_TITLE))?;
    // A task's name in the menu is its link's title.
    let props: IPropertyStore = link.cast()?;
    props.SetValue(&PKEY_Title, &PROPVARIANT::from(QUIT_TITLE))?;
    props.Commit()?;

    let tasks: IObjectCollection =
        CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
    tasks.AddObject(&link)?;
    let tasks: IObjectArray = tasks.cast()?;
    list.AddUserTasks(&tasks)?;
    list.CommitList()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_quit_flag_asks_to_quit() {
        assert!(wants_quit(["C:\\Lexpad\\Lexpad.exe", "--quit"]));
        assert!(!wants_quit(["C:\\Lexpad\\Lexpad.exe"]));
        assert!(!wants_quit(["C:\\Lexpad\\Lexpad.exe", "--player"]));
        // A film called --quit.mkv is a file to open, not the flag.
        assert!(!wants_quit(["Lexpad.exe", "C:\\films\\--quit.mkv"]));
        assert!(!wants_quit(["Lexpad.exe", "--quit-later"]));
    }

    #[test]
    fn the_task_has_the_tray_items_words() {
        assert_eq!(QUIT_TITLE, "Quit Lexpad");
        let tray = include_str!("tray.rs");
        assert!(
            tray.contains("\"Quit Lexpad\""),
            "the tray's Quit item and the task say the same"
        );
    }
}

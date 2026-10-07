// Keeps a console window from opening next to the app on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    lexpad_desktop_lib::run();
}

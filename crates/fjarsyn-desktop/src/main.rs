#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> fjarsyn_desktop::Result<()> {
    fjarsyn_desktop::run()
}

// A console window would flash on every launch of a GUI tool on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cursor_space_manager_lib::run()
}

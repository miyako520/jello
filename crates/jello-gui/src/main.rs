#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    jello_gui::run(std::env::args_os().nth(1))
}

// Hide the console window in release builds; debug keeps it for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rigel_app::run();
}

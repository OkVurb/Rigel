// Hide the console window in release builds; debug keeps it for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `rigel-app mcp ...` is the stdio server Claude calls back into; anything else opens the
    // window. A packaged app has no CLI on `PATH`, so the window binary hosts it itself.
    if let Some(code) = rigel_app::mcp_mode(std::env::args().skip(1)) {
        std::process::exit(code);
    }
    rigel_app::run();
}

// Stops console from showing, but also stops stdout and stderr
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod gui;
mod presets;
mod settings;
mod util;

use clap::Parser;
use gui::Gui;

fn main() {
    // Write panics to a log file so we can debug on Windows
    // (windows_subsystem = "windows" eats all stderr)
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{info}\n");
        let _ = std::fs::write("chloe-vibes-crash.log", &msg);
        // Also try stderr in case we have a console
        eprintln!("{msg}");
    }));

    let args = Gui::parse();
    gui::gui(args);
}

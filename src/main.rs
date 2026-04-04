// Stops console from showing, but also stops stdout and stderr
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod gui;
mod presets;
mod settings;
mod util;

use clap::Parser;
use gui::Gui;

fn crash_log_path() -> std::path::PathBuf {
    std::env::var("APPDATA")
        .map(|d| std::path::PathBuf::from(d).join("chloe-vibes"))
        .unwrap_or_else(|_| std::env::temp_dir().join("chloe-vibes"))
}

fn main() {
    // Write panics to a log file so we can debug on Windows
    // (windows_subsystem = "windows" eats all stderr)
    std::panic::set_hook(Box::new(|info| {
        let log_dir = crash_log_path();
        let _ = std::fs::create_dir_all(&log_dir);
        let log_file = log_dir.join("crash.log");
        // Rotate if crash log exceeds 1MB
        if let Ok(meta) = std::fs::metadata(&log_file) {
            if meta.len() > 1_000_000 {
                let _ = std::fs::rename(&log_file, log_dir.join("crash.log.old"));
            }
        }
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let msg = format!("[{timestamp}] {info}\n");
        // Append so we preserve earlier panics (cascade root cause)
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .and_then(|mut f| std::io::Write::write_all(&mut f, msg.as_bytes()));
        // Also try stderr in case we have a console
        eprintln!("{msg}");
    }));

    let args = Gui::parse();
    gui::gui(args);
}

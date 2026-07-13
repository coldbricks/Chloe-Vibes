// Stops console from showing, but also stops stdout and stderr
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod auto_lock;
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

fn session_log_path() -> std::path::PathBuf {
    crash_log_path().join("session.log")
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn session_log_append(msg: &str) {
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(session_log_path())
        .and_then(|mut f| std::io::Write::write_all(&mut f, msg.as_bytes()));
}

/// Heartbeat log that tells a real crash apart from a silent death:
/// a Rust panic leaves crash.log; a session that just vanishes (native
/// abort, external kill) leaves session.log ending in "alive Ns" with no
/// clean-exit marker; a normal close ends in "exit clean".
fn start_session_log() {
    let dir = crash_log_path();
    let _ = std::fs::create_dir_all(&dir);
    let log = session_log_path();
    // The previous session's tail is the evidence — preserve it unless
    // that session ended cleanly.
    if let Ok(prev) = std::fs::read_to_string(&log) {
        if !prev.trim_end().ends_with("exit clean") {
            let prev_path = dir.join("session.prev.log");
            let _ = std::fs::remove_file(&prev_path);
            let _ = std::fs::rename(&log, &prev_path);
        }
    }
    let _ = std::fs::write(
        &log,
        format!(
            "[{}] session start v{}\n",
            unix_now(),
            env!("CARGO_PKG_VERSION")
        ),
    );
    let _ = std::thread::Builder::new()
        .name("heartbeat".to_string())
        .spawn(|| {
            let start = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                session_log_append(&format!(
                    "[{}] alive {}s\n",
                    unix_now(),
                    start.elapsed().as_secs()
                ));
            }
        });
}

fn main() {
    // Write panics to a log file so we can debug on Windows
    // (windows_subsystem = "windows" eats all stderr)
    std::panic::set_hook(Box::new(|info| {
        // The capture thread intentionally catch_unwind's audio-capture crate
        // panics (WASAPI device blips). Those are recovered — do NOT spam
        // crash.log / backtraces or it looks like the app is dying every second.
        if std::thread::current().name() == Some("capture") {
            eprintln!("[capture] recovered panic (audio device blip): {info}");
            return;
        }

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
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Box<Any>".into()
        };
        let bt = std::backtrace::Backtrace::force_capture();
        let msg = format!("[{timestamp}] panic at {loc}: {payload}\n{bt}\n");
        // Append so we preserve earlier panics (cascade root cause)
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .and_then(|mut f| std::io::Write::write_all(&mut f, msg.as_bytes()));
        // Mark the session log too, so one file tells the whole story
        session_log_append(&format!(
            "[{timestamp}] PANIC at {loc} — details in crash.log\n"
        ));
        // Also try stderr in case we have a console
        eprintln!("{msg}");
    }));

    start_session_log();
    let args = Gui::parse();
    gui::gui(args);
    session_log_append(&format!("[{}] exit clean\n", unix_now()));
}

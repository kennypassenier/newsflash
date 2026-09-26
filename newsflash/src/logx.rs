//! journald-friendly logging (M11): sd-daemon priority prefixes on
//! stderr, one line per lifecycle event. No token ever passes through
//! here — asserted by the plaintext-scan test.
//!
//! Windows port: a desktop build has no journal, so `newsflash-win`
//! installs a file sink at startup. Linux never calls `set_sink` and
//! keeps the exact stderr lines above.

use std::sync::OnceLock;

/// Receives (sd-daemon priority, message).
pub type Sink = Box<dyn Fn(u8, &str) + Send + Sync>;

static SINK: OnceLock<Sink> = OnceLock::new();

/// First call wins; later calls are ignored (the sink is process-wide).
pub fn set_sink(sink: Sink) {
    let _ = SINK.set(sink);
}

fn emit(priority: u8, msg: &str) {
    match SINK.get() {
        Some(sink) => sink(priority, msg),
        None => eprintln!("<{priority}>{msg}"),
    }
}

pub fn info(msg: &str) {
    emit(6, msg);
}

pub fn warn(msg: &str) {
    emit(4, msg);
}

pub fn error(msg: &str) {
    emit(3, msg);
}

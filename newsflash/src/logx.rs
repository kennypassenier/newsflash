//! journald-friendly logging (M11): sd-daemon priority prefixes on
//! stderr, one line per lifecycle event. No token ever passes through
//! here — asserted by the plaintext-scan test.

pub fn info(msg: &str) {
    eprintln!("<6>{msg}");
}

pub fn warn(msg: &str) {
    eprintln!("<4>{msg}");
}

pub fn error(msg: &str) {
    eprintln!("<3>{msg}");
}

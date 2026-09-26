//! feat-win-6: hero images. Unpackaged Windows apps cannot point a toast at an
//! http URL — only at a local file — so the courier fetches the image
//! first (plain http on the LAN, the same no-TLS stance as the hub
//! client, AR2) and hands the toast a path. Every failure degrades to
//! "toast without image", never to a failed render: an image is
//! decoration, the text is the message.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The toast platform's own ceiling for a web image (3 MB).
pub const MAX_IMAGE_BYTES: u64 = 3 * 1024 * 1024;
/// Notification Center keeps toasts up to 3 days; their images must
/// outlive them, anything older is swept.
pub const KEEP_FOR: Duration = Duration::from_secs(3 * 24 * 3600);

/// Downloads `url` into `dir` as `<hub id>.<ext>` and returns the path.
pub fn fetch(url: &str, dir: &Path, hub_id: &str) -> Result<PathBuf, String> {
    if url.to_ascii_lowercase().starts_with("https://") {
        return Err(
            "https images are not supported — newsflash ships no TLS stack (AR2); \
             serve the image over plain http on the LAN"
                .into(),
        );
    }
    if !url.to_ascii_lowercase().starts_with("http://") {
        return Err(format!("image {url:?} is not an http:// URL"));
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(3))
        .timeout_read(Duration::from_secs(5))
        .build();
    let response = agent.get(url).call().map_err(|e| e.to_string())?;
    let ext = match response.content_type() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        other => return Err(format!("content-type {other:?} is not png/jpeg/gif")),
    };
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading image: {e}"))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err("image is larger than the 3 MB toast limit".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    sweep(dir, KEEP_FOR);
    let path = dir.join(format!("{}.{ext}", file_stem(hub_id)));
    std::fs::write(&path, &bytes).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

/// Hub ids are hub-controlled text; never let one escape the directory.
fn file_stem(hub_id: &str) -> String {
    let stem: String = hub_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if stem.is_empty() {
        "image".into()
    } else {
        stem
    }
}

/// Deletes cached images older than `keep`. Best effort, silent.
pub fn sweep(dir: &Path, keep: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > keep);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serve(content_type: &'static str, body: Vec<u8>) -> String {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", server.server_addr());
        std::thread::spawn(move || {
            if let Ok(req) = server.recv() {
                let header =
                    tiny_http::Header::from_bytes("content-type", content_type.as_bytes()).unwrap();
                let _ = req.respond(tiny_http::Response::from_data(body).with_header(header));
            }
        });
        addr
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nf-win-img-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn w6_a_png_is_cached_under_a_sanitized_hub_id() {
        let url = serve("image/png", b"\x89PNG fake".to_vec());
        let dir = temp("ok");
        let path = fetch(&format!("{url}/cam.png"), &dir, "../../evil\\id").unwrap();
        assert_eq!(path.parent().unwrap(), dir);
        assert_eq!(path.file_name().unwrap(), "______evil_id.png");
        assert_eq!(std::fs::read(&path).unwrap(), b"\x89PNG fake");
    }

    #[test]
    fn w6_non_images_and_oversized_images_are_refused() {
        let url = serve("text/html", b"<html>".to_vec());
        assert!(
            fetch(&url, &temp("html"), "1")
                .unwrap_err()
                .contains("text/html")
        );
        let url = serve("image/jpeg", vec![0; MAX_IMAGE_BYTES as usize + 10]);
        assert!(fetch(&url, &temp("big"), "2").unwrap_err().contains("3 MB"));
    }

    #[test]
    fn w6_https_and_other_schemes_are_refused_with_a_reason() {
        assert!(
            fetch("https://x/y.png", &temp("tls"), "3")
                .unwrap_err()
                .contains("AR2")
        );
        assert!(fetch("file:///C:/secret.png", &temp("file"), "4").is_err());
    }
}

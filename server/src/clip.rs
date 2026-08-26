use arboard::{ImageData, Clipboard};
use base64::Engine;
use std::borrow::Cow;
use tokio::sync::mpsc;

pub enum ClipCmd {
    SetText(String),
    /// `reply` reports whether the OS-level set actually succeeded.
    SetImage {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
        reply: tokio::sync::mpsc::UnboundedSender<bool>,
    },
    /// Read current clipboard; reply is PNG-encoded for images.
    GetPng(tokio::sync::mpsc::UnboundedSender<Option<ClipGet>>),
}

pub enum ClipGet {
    Text(String),
    Png { png: Vec<u8> },
}

/// Clipboard lives on its own OS thread because arboard::Clipboard is !Send.
/// Init retries forever: under systemd we may race the compositor startup,
/// and WAYLAND_DISPLAY can appear a few seconds after our process spawns.
pub fn spawn_clipboard() -> mpsc::Sender<ClipCmd> {
    let (tx, mut rx) = mpsc::channel::<ClipCmd>(64);
    std::thread::spawn(move || {
        let mut clip = loop {
            match Clipboard::new() {
                Ok(c) => break c,
                Err(e) => {
                    eprintln!("clipboard unavailable ({e}); retrying in 3s");
                    std::thread::sleep(std::time::Duration::from_secs(3));
                }
            }
        };
        while let Some(cmd) = rx.blocking_recv() {
            match cmd {
                ClipCmd::SetText(t) => {
                    if let Err(e) = clip.set_text(t) {
                        eprintln!("clipboard set_text failed: {e}");
                    }
                }
                ClipCmd::SetImage { width, height, rgba, reply } => {
                    // Piping large buffers into wl-copy from the parent proved
                    // racy on Hyprland; handing it a real file via shell
                    // redirect (the way humans run it) never failed.
                    let png = encode_png(width as u32, height as u32, &rgba);
                    let res = system_copy_png(&png);
                    let _ = reply.send(res.is_ok());
                    if let Err(e) = res {
                        eprintln!("clipboard set_image failed: {e}");
                    }
                }
                ClipCmd::GetPng(reply) => {
                    let _ = reply.send(read_clipboard_png(&mut clip));
                }
            }
        }
    });
    tx
}

fn read_clipboard_png(clip: &mut Clipboard) -> Option<ClipGet> {
    if let Ok(img) = clip.get_image() {
        let png = encode_png(img.width as u32, img.height as u32, &img.bytes);
        if png.is_empty() {
            return None;
        }
        return Some(ClipGet::Png { png });
    }
    clip.get_text().ok().filter(|t| !t.is_empty()).map(ClipGet::Text)
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let img = image::RgbaImage::from_raw(width.max(1), height.max(1), rgba.to_vec())
        .unwrap_or_else(|| image::RgbaImage::new(1, 1));
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok();
    out
}

/// Put PNG bytes on the system clipboard. On Wayland we mimic the shell
/// invocation `wl-copy -t image/png < file`, which is reliable on Hyprland,
/// by writing a temp file and letting sh perform the redirect. Falls back
/// to xclip on X11.
fn system_copy_png(png: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();

    if wayland {
        let dir = std::env::temp_dir();
        cleanup_stale_clip_files(&dir);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = dir.join(format!("sender-clip-{stamp}.png"));
        std::fs::write(&path, png).map_err(|e| format!("temp write: {e}"))?;

        // Detached so the owner survives our process; the temp file is
        // reaped later by cleanup_stale_clip_files, not right away.
        let cmd = format!(
            "setsid wl-copy -t image/png < '{}' >/dev/null 2>&1 &",
            path.display()
        );
        return match Command::new("sh").arg("-c").arg(&cmd).status() {
            Ok(s) if s.success() => {
                // give the owner a moment, then confirm it's really serving
                std::thread::sleep(std::time::Duration::from_millis(300));
                let alive = Command::new("pgrep")
                    .arg("-x")
                    .arg("wl-copy")
                    .output()
                    .map(|o| !o.stdout.is_empty())
                    .unwrap_or(false);
                if alive {
                    Ok(())
                } else {
                    Err("wl-copy did not stay alive".into())
                }
            }
            Ok(_) => Err("sh reported failure".into()),
            Err(e) => Err(format!("sh spawn: {e}")),
        };
    }

    match Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "image/png", "-i"])
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(mut child) => {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                if stdin.write_all(png).is_ok() && stdin.flush().is_ok() {
                    drop(child.stdin.take());
                    return Ok(());
                }
            }
            let _ = child.kill();
            Err("xclip pipe failed".into())
        }
        Err(e) => Err(format!("xclip spawn: {e}")),
    }
}

/// Remove leftover clipboard temp files older than 10 minutes.
fn cleanup_stale_clip_files(dir: &std::path::Path) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("sender-clip-") && name.ends_with(".png") {
                if let Ok(md) = e.metadata() {
                    let age = md
                        .modified()
                        .ok()
                        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(now);
                    if now.saturating_sub(age) > 600 {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
    }
}

/// Decode a base64 data payload into RGBA suitable for the system clipboard.
/// Very large images (phone photos) are downscaled to keep the clipboard
/// payload manageable; the untouched original is saved separately by callers.
pub fn decode_to_rgba(mime: &str, data_b64: &str) -> Result<(usize, usize, Vec<u8>), String> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(data_b64.trim())
        .map_err(|e| format!("base64: {e}"))?;
    let fmt = match mime.to_ascii_lowercase().as_str() {
        "image/png" => image::ImageFormat::Png,
        "image/jpeg" | "image/jpg" => image::ImageFormat::Jpeg,
        "image/webp" => image::ImageFormat::WebP,
        "image/gif" => image::ImageFormat::Gif,
        "image/bmp" => image::ImageFormat::Bmp,
        other => return Err(format!("unsupported mime {other}")),
    };
    let img = image::load_from_memory_with_format(&raw, fmt)
        .map_err(|e| format!("decode: {e}"))?;
    let mut img = img.to_rgba8();
    const MAX_SIDE: u32 = 2560;
    let (w, h) = img.dimensions();
    if w.max(h) > MAX_SIDE {
        let (nw, nh) = if w >= h {
            (MAX_SIDE, (h as f64 * MAX_SIDE as f64 / w as f64).round() as u32)
        } else {
            ((w as f64 * MAX_SIDE as f64 / h as f64).round() as u32, MAX_SIDE)
        };
        img = image::imageops::resize(&img, nw.max(1), nh.max(1), image::imageops::FilterType::Triangle);
    }
    let (w, h) = img.dimensions();
    Ok((w as usize, h as usize, img.into_raw()))
}

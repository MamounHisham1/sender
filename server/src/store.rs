use std::fs;
use std::io;
use std::path::PathBuf;

fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("sender")
}

/// Stable 6-digit PIN: generated once, reused across runs so the phone
/// only ever has to enter it a single time.
pub fn load_or_create_pin() -> io::Result<String> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    let file = dir.join("config.json");
    if file.exists() {
        if let Ok(txt) = fs::read_to_string(&file) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                if let Some(p) = v["pin"].as_str() {
                    return Ok(p.to_string());
                }
            }
        }
    }
    use rand::Rng;
    let pin = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000));
    fs::write(&file, serde_json::json!({ "pin": pin }).to_string())?;
    Ok(pin)
}

pub fn inbox_dir() -> PathBuf {
    let p = dirs::picture_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join("Pictures"))
        .join("Sender");
    let _ = fs::create_dir_all(&p);
    p
}

pub fn sanitize_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect();
    let trimmed = clean.trim_matches('_');
    if trimmed.is_empty() { "image".into() } else { trimmed.to_string() }
}

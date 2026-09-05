use qrcode::{EcLevel, QrCode};

/// Canonical pairing payload scanned by the phone, e.g.
/// `sender://pair?host=192.168.1.20:8787&pin=123456`.
pub fn pair_url(host_with_port: &str, pin: &str) -> String {
    format!("sender://pair?host={host_with_port}&pin={pin}")
}

/// Render payload as compact terminal rows using half-blocks
/// (2 QR modules per terminal cell). No ANSI colors — the caller
/// decides styling (TUI shows it black-on-white for scannability).
pub fn qr_lines(payload: &str) -> Vec<String> {
    match QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M) {
        Ok(code) => code
            .render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .module_dimensions(1, 1)
            .build()
            .lines()
            .map(|l| l.to_string())
            .collect(),
        Err(_) => vec![payload.to_string()],
    }
}

/// Same as qr_lines but as a single printable block for --headless.
pub fn qr_text(payload: &str) -> String {
    qr_lines(payload).join("\n")
}

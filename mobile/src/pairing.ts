import { normalizeHost } from './config';

/** Canonical payload shown by the laptop, e.g. `sender://pair?host=192.168.1.20:8787&pin=123456`. */
export function buildPairUrl(hostWithPort: string, pin: string): string {
  return `sender://pair?host=${hostWithPort}&pin=${pin}`;
}

export interface PairingParsed {
  host: string;
  pin: string;
}

/**
 * Parse a scanned QR payload. Accepts the canonical `sender://pair?...`
 * URL plus a JSON fallback (`{"host","pin"}`). Returns null when invalid.
 */
export function parsePairPayload(raw: string): PairingParsed | null {
  const s = raw.trim();

  // JSON fallback: {"host":"192.168.1.20:8787","pin":"123456"}
  if (s.startsWith('{')) {
    try {
      const v = JSON.parse(s);
      const host = typeof v.host === 'string' ? normalizeHost(v.host) : '';
      const pin = typeof v.pin === 'string' ? v.pin : '';
      if (host && /^\d{6}$/.test(pin)) return { host, pin };
    } catch {}
    return null;
  }

  // sender://pair?host=...&pin=... (also tolerate https:// or bare query)
  const q = s.includes('?') ? s.slice(s.indexOf('?') + 1) : s;
  const params = new URLSearchParams(q);
  const hostRaw = params.get('host') ?? '';
  const pin = (params.get('pin') ?? '').trim();
  if (!hostRaw || !/^\d{6}$/.test(pin)) return null;
  const host = normalizeHost(hostRaw);
  if (!host) return null;
  return { host, pin };
}

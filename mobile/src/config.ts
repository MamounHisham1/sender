import AsyncStorage from '@react-native-async-storage/async-storage';

const KEY_HOST = 'sender.host';
const KEY_PIN = 'sender.pin';

export interface Pairing {
  host: string | null; // "192.168.1.20:8787"
  pin: string | null;
}

export async function loadPairing(): Promise<Pairing> {
  const [host, pin] = await Promise.all([
    AsyncStorage.getItem(KEY_HOST),
    AsyncStorage.getItem(KEY_PIN),
  ]);
  return { host, pin };
}

export async function savePin(pin: string): Promise<void> {
  await AsyncStorage.setItem(KEY_PIN, pin);
}

export async function saveHost(host: string): Promise<void> {
  await AsyncStorage.setItem(KEY_HOST, host);
}

/**
 * Derive the laptop's LAN address from Expo Go's dev-server info.
 * Depending on SDK version the value hides in different spots and may carry
 * the Metro port (8081/19000); we only need the IP, then apply our own port.
 */
export function laptopHostFromExpo(): string | null {
  try {
    // Lazy require so this module stays usable in plain Node tests.
    const Constants = require('expo-constants');
    const mod = Constants?.default ?? Constants;
    const candidates: unknown[] = [
      mod?.expoConfig?.hostUri,
      mod?.hostUri,
      mod?.manifest?.developer?.hostUri,
      mod?.manifest?.debuggerHost,
      typeof mod?.getManifest === 'function' ? mod.getManifest()?.developer?.hostUri : null,
    ];
    for (const cand of candidates) {
      if (typeof cand === 'string' && /^\d{1,3}(\.\d{1,3}){3}/.test(cand)) {
        const ip = cand.split(':')[0];
        return `${ip}:8787`;
      }
    }
  } catch {}
  return null;
}

/**
 * Accepts "192.168.1.20:8787", "192.168.1.20", "ws://192.168.1.20:8787",
 * or "localhost:8081". Loopback hosts are useless from the phone, so they
 * are kept only if nothing better exists (user can still edit manually).
 */
export function normalizeHost(input: string): string {
  let s = input.trim().replace(/^wss?:\/\//i, '').replace(/\/+$/, '');
  if (!/:\d+$/.test(s)) s = `${s}:8787`;
  return s;
}

export function isLoopback(host: string): boolean {
  const ip = host.split(':')[0];
  return ip === 'localhost' || ip === '127.0.0.1';
}

export function wsUrlFor(host: string): string {
  return `ws://${host}/ws`;
}

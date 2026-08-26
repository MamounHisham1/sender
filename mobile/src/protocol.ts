// Wire protocol — must stay in sync with server/src/proto.rs
export type Msg =
  | { type: 'hello'; pin: string; name: string }
  | { type: 'welcome'; ok: boolean; err?: string | null }
  | { type: 'text'; id: string; body: string; ts: number }
  | { type: 'img'; id: string; name: string; mime: string; data: string; ts: number }
  | { type: 'ack'; id: string; ok: boolean; err?: string | null }
  | { type: 'ping' }
  | { type: 'pong' };

export function encode(m: Msg): string {
  return JSON.stringify(m);
}

export function decode(raw: string): Msg | null {
  try {
    const v = JSON.parse(raw);
    if (v && typeof v === 'object' && typeof v.type === 'string') return v as Msg;
    return null;
  } catch {
    return null;
  }
}

export function newId(): string {
  return `${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
}

// Simulated phone client for testing the Sender server end-to-end.
// Usage: node test-client.mjs <pin> [--badpin]
import WebSocket from 'ws';
import { readFileSync } from 'node:fs';

const pin = process.argv[2];
const badpin = process.argv.includes('--badpin');
const listenOnly = process.argv.includes('--listen-only');
if (!pin) {
  console.error('usage: node test-client.mjs <pin> [--badpin] [--listen-only]');
  process.exit(1);
}

const host = process.env.SENDER_HOST ?? '127.0.0.1:8787';
const url = `ws://${host}/ws`;
const log = (...a) => console.log('[phone]', ...a);

const ws = new WebSocket(url);

const send = (obj) => ws.send(JSON.stringify(obj));

ws.on('open', () => {
  log('connected to', url);
  send({ type: 'hello', pin: badpin ? '000000'.replace(/0/g, '9') === pin ? '000000' : '999999' : pin, name: 'TestPhone' });
});

ws.on('message', (raw) => {
  let msg;
  try { msg = JSON.parse(raw.toString()); } catch { return; }
  log('◀', JSON.stringify(msg).slice(0, 120));

  switch (msg.type) {
    case 'welcome':
      if (!msg.ok) {
        log('✗ server rejected our PIN — closing');
        process.exit(2);
      }
      log('✓ paired! sending text…');
      if (listenOnly) {
        log('(listen-only mode, waiting for laptop messages)');
        return;
      }
      send({ type: 'text', id: 't1', body: 'hello from the fake phone', ts: Date.now() });
      setTimeout(() => {
        log('sending image…');
        const b64 = readFileSync(new URL('./test-image.png', import.meta.url)).toString('base64');
        send({ type: 'img', id: 'i1', name: 'test.png', mime: 'image/png', data: b64, ts: Date.now() });
      }, 300);
      break;
    case 'text':
      log('◀ text from laptop:', msg.body);
      break;
    case 'img':
      log('◀ image from laptop:', msg.name, msg.data.length, 'b64 chars');
      break;
  }
});

ws.on('close', (code) => { log('closed', code); process.exit(0); });
ws.on('error', (e) => { log('error', e.message); process.exit(1); });

// exit after a few seconds
setTimeout(() => { log('done'); process.exit(0); }, 4000);

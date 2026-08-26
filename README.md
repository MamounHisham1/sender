# Sender — phone ⇄ laptop sync

Send text and images between your Android phone and laptop over Wi-Fi.
Anything received on either side lands **directly in that device's clipboard**
— no clicks needed.

- **Laptop:** a Rust TUI. Type + Enter sends text; `p` pushes your clipboard
  (image or text) to the phone; received texts/images go straight into the
  clipboard (images also saved under `~/Pictures/Sender/`).
- **Phone:** an Expo Go app. Type + Send, pick 🖼 or shoot 📷; long-press any
  bubble to copy it into the phone's clipboard.

## Pairing (one time)

1. On the laptop:
   ```
   cd server && cargo run --release
   ```
   A TUI shows `ws://<laptop-ip>:8787` and a 6-digit PIN (stable across runs,
   stored in `~/.config/sender/config.json`).
2. On the phone: install **Expo Go**, then scan the QR code from
   `cd mobile && npx expo start`. The app auto-detects the laptop's IP from
   Expo's dev server; enter the PIN once.
3. Done — after that just keep both apps running on the same Wi-Fi.

## Day-to-day

```
# laptop
cargo run --release          # in server/
```

```
# phone: open Sender inside Expo Go   (or `npx expo start` again)
```

## Protocol notes

- Single WebSocket (`ws://laptop:8787/ws`); all messages are JSON with a
  `type` tag (`hello/welcome/text/img/ack/ping/pong`) — see
  `server/src/proto.rs` and `mobile/src/protocol.ts`.
- First message must be `{type:"hello", pin}`; wrong PIN ⇒ connection closed.
- Images travel as base64 JSON frames; 25 MB cap per image.
- Phone auto-reconnects with backoff; the laptop accepts reconnects forever.
- Headless server mode for testing: `sender-server --headless [--send "txt"]`.
- Fake-phone test client: `node server/test-client.mjs <pin> [--listen-only]`.

## Layout

```
server/    Rust TUI (tokio + tungstenite + arboard + ratatui)
mobile/    Expo app (TypeScript), runs inside Expo Go
```

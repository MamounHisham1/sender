mod clip;
mod proto;
mod qr;
mod store;

use clip::{ClipCmd, ClipGet, spawn_clipboard};
use futures_util::{SinkExt, StreamExt};
use proto::{Msg, new_id, now_ms, to_json};
use ratatui::{
    Frame,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message as WsMsg;

const PORT: u16 = 8787;

#[derive(Debug, Clone)]
enum UiEvent {
    Log(String),
    Connected(String),
    Disconnected,
}

struct State {
    phone: Option<String>,
    log: VecDeque<String>,
    input: String,
    show_qr: bool,
}

impl State {
    fn log(&mut self, s: impl Into<String>) {
        self.log.push_back(s.into());
        while self.log.len() > 400 {
            self.log.pop_front();
        }
    }
}

fn fallback_local_ip() -> String {
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                return addr.ip().to_string();
            }
        }
    }
    "127.0.0.1".into()
}

fn local_ip() -> String {
    local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| fallback_local_ip())
}

// --- section: startup ---

fn main() -> io::Result<()> {
    let headless = std::env::args().any(|a| a == "--headless");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    if headless {
        rt.block_on(run_headless())
    } else {
        rt.block_on(run())
    }
}

async fn run_headless() -> io::Result<()> {
    let pin = store::load_or_create_pin()?;
    let ip = local_ip();
    let addr = format!("ws://{ip}:{PORT}");
    let host_with_port = format!("{ip}:{PORT}");
    let pair = qr::pair_url(&host_with_port, &pin);

    let (out_tx, _) = broadcast::channel::<Msg>(128);
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<UiEvent>();
    let clip_tx = spawn_clipboard();
    let _ = CLIP_TX.set(clip_tx.clone());

    spawn_listener(out_tx.clone(), ui_tx.clone(), pin.clone()).await;

    let connected = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // --send "text": fire one message at the phone once it connects (testing/scripting).
    let send_now: Option<String> = std::env::args()
        .position(|a| a == "--send")
        .and_then(|i| std::env::args().nth(i + 1));
    if let Some(text) = send_now {
        let out_tx2 = out_tx.clone();
        let connected3 = connected.clone();
        std::thread::spawn(move || loop {
            if connected3.load(std::sync::atomic::Ordering::Relaxed) {
                println!("you ▸ {text}");
                let _ = out_tx2.send(Msg::Text { id: new_id(), body: text, ts: now_ms() });
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        });
    }

    println!("pair url  {addr}");
    println!("pin       {pin}");
    println!("qr        {pair}");
    println!("{}", qr::qr_text(&pair));
    println!("type a line + Enter to send it to the phone; Ctrl-D to quit");

    // stdin → phone (only for interactive terminals; piped/empty stdin would EOF instantly)
    if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        let send_tx = out_tx.clone();
        let connected2 = connected.clone();
        std::thread::spawn(move || {
            let mut line = String::new();
            loop {
                line.clear();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let body = line.trim().to_string();
                        if body.is_empty() {
                            continue;
                        }
                        if !connected2.load(std::sync::atomic::Ordering::Relaxed) {
                            println!("(no phone connected)");
                            continue;
                        }
                        println!("you ▸ {body}");
                        let _ = send_tx.send(Msg::Text { id: new_id(), body, ts: now_ms() });
                    }
                }
            }
        });
    }

    while let Some(ev) = ui_rx.recv().await {
        match ev {
            UiEvent::Log(s) => println!("{s}"),
            UiEvent::Connected(name) => {
                connected.store(true, std::sync::atomic::Ordering::Relaxed);
                println!("✓ phone connected: {name}");
            }
            UiEvent::Disconnected => {
                connected.store(false, std::sync::atomic::Ordering::Relaxed);
                println!("phone disconnected");
            }
        }
    }
    Ok(())
}

async fn spawn_listener(out_tx: broadcast::Sender<Msg>, ui_tx: mpsc::UnboundedSender<UiEvent>, pin: String) {
    tokio::spawn(async move {
        let listener = match TcpListener::bind(("0.0.0.0", PORT)).await {
            Ok(l) => l,
            Err(e) => {
                let _ = ui_tx.send(UiEvent::Log(format!("FATAL: cannot bind port {PORT}: {e}")));
                return;
            }
        };
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    let out_tx = out_tx.clone();
                    let ui_tx = ui_tx.clone();
                    let pin = pin.clone();
                    tokio::spawn(async move {
                        handle_connection(stream, peer, pin, out_tx, ui_tx).await;
                    });
                }
                Err(e) => {
                    let _ = ui_tx.send(UiEvent::Log(format!("accept error: {e}")));
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
            }
        }
    });
}

async fn run() -> io::Result<()> {
    let pin = store::load_or_create_pin()?;
    let ip = local_ip();
    let addr = format!("ws://{ip}:{PORT}");
    let pair = qr::pair_url(&format!("{ip}:{PORT}"), &pin);

    let (out_tx, _) = broadcast::channel::<Msg>(128);
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<UiEvent>();
    let clip_tx = spawn_clipboard();
    let _ = CLIP_TX.set(clip_tx.clone());

    // Accept phone connections forever; reconnects are cheap.
    spawn_listener(out_tx.clone(), ui_tx.clone(), pin.clone()).await;

    // Blocking keyboard reader on its own OS thread.
    let (key_tx, mut key_rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || loop {
        match crossterm::event::read() {
            Ok(ev) => {
                if key_tx.send(ev).is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    });

    let mut terminal = init_terminal()?;
    let mut st = State { phone: None, log: VecDeque::new(), input: String::new(), show_qr: true };
    st.log(format!("pair url  {addr}"));
    st.log(format!("pin       {pin}"));
    st.log("scan the QR on the right (r toggles) or enter PIN manually".to_string());
    let res = tui_loop(
        &mut terminal, &mut st, &addr, &pin, &ip, &pair,
        out_tx.clone(), ui_tx.clone(), clip_tx.clone(),
        &mut ui_rx, &mut key_rx,
    )
    .await;
    restore_terminal(&mut terminal)?;
    res
}

fn init_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    let mut stdout = io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen, crossterm::event::EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen, crossterm::event::DisableMouseCapture)?;
    crossterm::terminal::disable_raw_mode()?;
    terminal.show_cursor()?;
    Ok(())
}

// --- section: tui ---

#[allow(clippy::too_many_arguments)]
async fn tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    st: &mut State,
    addr: &str,
    pin: &str,
    ip: &str,
    pair: &str,
    out_tx: broadcast::Sender<Msg>,
    ui_tx: mpsc::UnboundedSender<UiEvent>,
    clip_tx: mpsc::Sender<ClipCmd>,
    ui_rx: &mut mpsc::UnboundedReceiver<UiEvent>,
    key_rx: &mut mpsc::UnboundedReceiver<crossterm::event::Event>,
) -> io::Result<()> {
    use crossterm::event::{Event as Ev, KeyCode, KeyEventKind, KeyModifiers};

    let mut tick = tokio::time::interval(std::time::Duration::from_millis(250));

    loop {
        terminal.draw(|f| draw(f, st, addr, pin, ip, pair))?;

        tokio::select! {
            maybe_ev = key_rx.recv() => {
                let Some(Ev::Key(key)) = maybe_ev else { continue };
                if key.kind != KeyEventKind::Press { continue; }
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('q') if st.input.is_empty() => break,
                    KeyCode::Esc if st.input.is_empty() => break,
                    KeyCode::Enter => {
                        let body = st.input.trim().to_string();
                        if body.is_empty() { continue; }
                        st.input.clear();
                        if st.phone.is_none() {
                            st.log("no phone connected yet");
                            continue;
                        }
                        st.log(format!("you ▸ {body}"));
                        let _ = out_tx.send(Msg::Text { id: new_id(), body, ts: now_ms() });
                    }
                    KeyCode::Backspace => { st.input.pop(); }
                    KeyCode::Char('r') if st.input.is_empty() => {
                        st.show_qr = !st.show_qr;
                    }
                    KeyCode::Char('p') if st.input.is_empty() => {
                        if st.phone.is_none() {
                            st.log("no phone connected yet");
                            continue;
                        }
                        push_clipboard(&clip_tx, &out_tx, &ui_tx).await;
                    }
                    KeyCode::Char(c) => st.input.push(c),
                    _ => {}
                }
            }
            Some(ev) = ui_rx.recv() => match ev {
                UiEvent::Log(s) => st.log(s),
                UiEvent::Connected(name) => {
                    st.phone = Some(name.clone());
                    st.log(format!("✓ phone connected: {name}"));
                }
                UiEvent::Disconnected => {
                    if st.phone.take().is_some() {
                        st.log("phone disconnected");
                    }
                }
            },
            _ = tick.tick() => {}
        }
    }
    Ok(())
}

/// Read the laptop clipboard and beam it to the phone (image or text).
async fn push_clipboard(
    clip_tx: &mpsc::Sender<ClipCmd>,
    out_tx: &broadcast::Sender<Msg>,
    ui_tx: &mpsc::UnboundedSender<UiEvent>,
) {
    // arboard is !Send; hop through the dedicated clipboard thread.
    let (rep_tx, mut rep_rx) = mpsc::unbounded_channel::<Option<ClipGet>>();
    if clip_tx.send(ClipCmd::GetPng(rep_tx)).await.is_err() {
        return;
    }
    let got = tokio::time::timeout(std::time::Duration::from_secs(2), rep_rx.recv()).await;
    match got.ok().flatten().flatten() {
        Some(ClipGet::Text(t)) => {
            if t.trim().is_empty() {
                let _ = ui_tx.send(UiEvent::Log("clipboard empty".into()));
                return;
            }
            let preview: String = t.chars().take(48).collect();
            let _ = ui_tx.send(UiEvent::Log(format!("you ▸ [clip] {preview}")));
            let _ = out_tx.send(Msg::Text { id: new_id(), body: t, ts: now_ms() });
        }
        Some(ClipGet::Png { png, .. }) => {
            if png.is_empty() {
                let _ = ui_tx.send(UiEvent::Log("clipboard empty".into()));
                return;
            }
            let id = new_id();
            let _ = ui_tx.send(UiEvent::Log(format!("you ▸ [image {} KB]", png.len() / 1024)));
            let _ = out_tx.send(Msg::Img {
                name: format!("clip-{id}.png"),
                mime: "image/png".into(),
                data: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png),
                id,
                ts: now_ms(),
            });
        }
        None => {
            let _ = ui_tx.send(UiEvent::Log("could not read clipboard".into()));
        }
    }
}

// --- section: draw ---

fn draw(f: &mut Frame, st: &State, addr: &str, pin: &str, ip: &str, pair: &str) {
    let [header, body, input_area, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(f.area());

    let status = match &st.phone {
        Some(name) => Span::styled(
            format!(" ● {name} "),
            Style::default().fg(Color::Black).bg(Color::Green),
        ),
        None => Span::styled(
            " ○ waiting for phone ",
            Style::default().fg(Color::Black).bg(Color::Yellow),
        ),
    };
    let header_w = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                " Sender ",
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(addr.to_string(), Style::default().fg(Color::Cyan)),
            Span::raw("   PIN: "),
            Span::styled(pin.to_string(), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw(" laptop ip: "),
            Span::styled(ip.to_string(), Style::default().fg(Color::Blue)),
            status,
        ]),
    ])
    .block(Block::new().borders(Borders::ALL));
    f.render_widget(header_w, header);

    let lines: Vec<Line> = st
        .log
        .iter()
        .map(|l| {
            let style = if l.starts_with('✓') {
                Style::default().fg(Color::Green)
            } else if l.starts_with('✗') || l.starts_with("FATAL") {
                Style::default().fg(Color::Red)
            } else if l.starts_with("you") {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            Line::styled(l.clone(), style)
        })
        .collect();
    // QR panel on the right (toggle with `r`). Black-on-white so phones
    // can scan it straight off the screen.
    let qr_rows = qr::qr_lines(pair);
    let qr_w = qr_rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as u16;
    let show_qr = st.show_qr && qr_w > 0 && f.area().width >= qr_w + 50;
    if show_qr {
        let [log_area, qr_area] =
            Layout::horizontal([Constraint::Min(10), Constraint::Length(qr_w + 4)])
                .areas(body);
        f.render_widget(
            Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
            log_area.inner(Margin { vertical: 0, horizontal: 1 }),
        );
        let qr_wd = Paragraph::new(qr_rows.join("\n"))
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::Black).bg(Color::White))
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(" scan to pair ")
                    .style(Style::default().bg(Color::White)),
            );
        f.render_widget(qr_wd, qr_area);
    } else {
        f.render_widget(
            Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
            body.inner(Margin { vertical: 0, horizontal: 1 }),
        );
    }

    let input = Paragraph::new(format!("{}▏", st.input))
        .block(Block::new().borders(Borders::ALL).title(" text to phone "));
    f.render_widget(input, input_area);

    let hint = if st.phone.is_some() {
        "type + Enter send · p push clipboard · r QR · q quit"
    } else {
        "scan QR or type PIN on phone · p push clipboard · r QR · q quit"
    };
    f.render_widget(Span::styled(hint, Style::default().fg(Color::DarkGray)), footer);
}

// --- section: ws ---

async fn handle_connection(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    pin: String,
    out_tx: broadcast::Sender<Msg>,
    ui_tx: mpsc::UnboundedSender<UiEvent>,
) {
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(w) => w,
        Err(e) => {
            let _ = ui_tx.send(UiEvent::Log(format!("handshake failed {peer}: {e}")));
            return;
        }
    };
    let (mut sink, mut source) = ws.split();
    let mut out_rx = out_tx.subscribe();

    // ---- auth: expect Hello{pin} as the very first message ----
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), source.next()).await;
    let ok = matches!(
        first,
        Ok(Some(Ok(WsMsg::Text(txt))))
            if matches!(serde_json::from_str::<Msg>(&txt),
                Ok(Msg::Hello { pin: got, .. }) if constant_eq(&got, &pin))
    );
    let welcome = Msg::Welcome {
        ok,
        err: (!ok).then(|| "bad PIN".to_string()),
    };
    if sink.send(WsMsg::Text(to_json(&welcome).into())).await.is_err() {
        return;
    }
    if !ok {
        let _ = ui_tx.send(UiEvent::Log(format!("✗ rejected {peer} (bad PIN)")));
        return;
    }
    let _ = ui_tx.send(UiEvent::Connected(peer.to_string()));

    // ---- relay loop ----
    loop {
        tokio::select! {
            inbound = source.next() => match inbound {
                Some(Ok(WsMsg::Text(txt))) => {
                    match serde_json::from_str::<Msg>(&txt) {
                        Ok(msg) => {
                            if !handle_inbound(msg, &clip_tx_for(&ui_tx), &ui_tx).await {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = ui_tx.send(UiEvent::Log(format!("bad msg: {e}")));
                        }
                    }
                }
                Some(Ok(WsMsg::Ping(p))) => {
                    let _ = sink.send(WsMsg::Pong(p)).await;
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => {
                    let _ = ui_tx.send(UiEvent::Disconnected);
                    break;
                }
            },
            out = out_rx.recv() => match out {
                Ok(msg) => {
                    if sink.send(WsMsg::Text(to_json(&msg).into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let _ = ui_tx.send(UiEvent::Log(format!("slow consumer, dropped {n}")));
                }
                Err(_) => break,
            },
        }
    }
}

// The clipboard channel is created in run(); stash it here so WS handlers can reach it.
static CLIP_TX: std::sync::OnceLock<mpsc::Sender<ClipCmd>> = std::sync::OnceLock::new();

fn clip_tx_for(_ui: &mpsc::UnboundedSender<UiEvent>) -> &'static mpsc::Sender<ClipCmd> {
    CLIP_TX.get().expect("clipboard channel initialized")
}

async fn handle_inbound(
    msg: Msg,
    clip: &mpsc::Sender<ClipCmd>,
    ui_tx: &mpsc::UnboundedSender<UiEvent>,
) -> bool {
    match msg {
        Msg::Text { body, .. } => {
            let _ = clip.send(ClipCmd::SetText(body.clone())).await;
            let _ = ui_tx.send(UiEvent::Log(format!("phone ▸ {body}")));
            true
        }
        Msg::Img { name, mime, data, .. } => {
            match handle_image(&name, &mime, &data).await {
                Ok((path, kb, clip_ok)) => {
                    let clip_note = if clip_ok { "clipboard ✓" } else { "CLIPBOARD ✗" };
                    let _ = ui_tx.send(UiEvent::Log(format!(
                        "phone ▸ [image] saved {} ({} KB), {}",
                        path.display(),
                        kb,
                        clip_note
                    )));
                    true
                }
                Err(e) => {
                    let _ = ui_tx.send(UiEvent::Log(format!("✗ image failed: {e}")));
                    true
                }
            }
        }
        Msg::Ping | Msg::Pong | Msg::Ack { .. } => true,
        Msg::Hello { .. } | Msg::Welcome { .. } => true,
    }
}

async fn handle_image(
    name: &str,
    mime: &str,
    data_b64: &str,
) -> Result<(std::path::PathBuf, usize, bool), String> {
    use base64::Engine;

    let raw = base64::engine::general_purpose::STANDARD
        .decode(data_b64.trim())
        .map_err(|e| format!("base64: {e}"))?;
    if raw.len() > 25 * 1024 * 1024 {
        return Err("image too large".into());
    }

    // Save the original bytes to disk first — that must never fail silently.
    let dir = store::inbox_dir();
    let fname = format!("{}-{}", now_ms(), store::sanitize_name(name));
    let path = dir.join(fname);
    std::fs::write(&path, &raw).map_err(|e| format!("save: {e}"))?;

    // Then try the clipboard; wait for the true OS-level result.
    let clip_ok = match crate::clip::decode_to_rgba(mime, data_b64) {
        Ok((w, h, rgba)) => {
            let (rep_tx, mut rep_rx) = tokio::sync::mpsc::unbounded_channel::<bool>();
            let sent = CLIP_TX
                .get()
                .map(|c| c.try_send(ClipCmd::SetImage { width: w, height: h, rgba, reply: rep_tx }))
                .transpose()
                .is_ok();
            // clipboard thread normally answers in microseconds; cap at 2s
            let got = tokio::time::timeout(std::time::Duration::from_secs(2), rep_rx.recv()).await;
            sent && matches!(got, Ok(Some(true)))
        }
        Err(_) => false,
    };

    Ok((path, raw.len() / 1024, clip_ok))
}

fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}



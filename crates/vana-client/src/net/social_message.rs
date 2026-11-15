//! Shared Banshee message model. Display conversion never changes the outgoing
//! body. The launcher owns the social.chat credential; HTTP errors and messages are never logged.

pub const MESSAGE_CHARACTERS: usize = 1000;
pub const BOT_CHARACTERS: usize = 200;
pub const DISPLAY_BYTES: usize = 150;

/// Keep this value for every retry of one send; create another only for a new
/// message. The transport supplies 16 bytes from the OS random source.
#[derive(Clone)]
pub struct PendingMessage {
    pub to: String,
    pub body: String,
    pub client_id: String,
}

impl std::fmt::Debug for PendingMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingMessage { to: <redacted>, body: <redacted>, client_id: <redacted> }")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidMessage {
    Recipient,
    Empty,
    TooLong,
}

impl PendingMessage {
    pub fn new(to: &str, body: String, bot: bool, nonce: [u8; 16]) -> Result<Self, InvalidMessage> {
        if to.trim().is_empty() || to.chars().any(char::is_control) {
            return Err(InvalidMessage::Recipient);
        }
        if body.trim().is_empty() {
            return Err(InvalidMessage::Empty);
        }
        if body.chars().count() > if bot { BOT_CHARACTERS } else { MESSAGE_CHARACTERS } {
            return Err(InvalidMessage::TooLong);
        }
        let client_id = nonce.iter().map(|b| format!("{b:02x}")).collect();
        Ok(Self { to: to.to_owned(), body, client_id })
    }
}

/// Printable ASCII only: control bytes (including FFXI text commands), newlines
/// and Unicode formatting controls cannot create extra chat lines. Unicode text
/// is retained in the outgoing body and represented by '?' only when displayed.
pub fn display_ascii(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else if c.is_whitespace() || c.is_control() {
                ' '
            } else {
                '?'
            }
        })
        .collect()
}

/// Each line includes its sender, including continuations. Bound the sender so
/// even an invalid remote name cannot exhaust the line or impersonate a prefix.
pub fn tell_lines(sender: &str, body: &str) -> Vec<String> {
    display_lines(sender, body, DISPLAY_BYTES)
}

pub fn display_lines(sender: &str, body: &str, limit: usize) -> Vec<String> {
    let name: String = display_ascii(sender)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '_' })
        .take(32)
        .collect();
    let prefix = format!("{}>> ", if name.is_empty() { "Unknown" } else { &name });
    let width = limit.clamp(40, DISPLAY_BYTES) - prefix.len();
    let text = display_ascii(body);
    let mut rest = text.trim();
    let mut lines = Vec::new();
    while !rest.is_empty() {
        let end =
            if rest.len() <= width { rest.len() } else { rest[..width].rfind(' ').filter(|&i| i > 0).unwrap_or(width) };
        lines.push(format!("{prefix}{}", rest[..end].trim_end()));
        rest = rest[end..].trim_start();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_ascii_bounded_and_preserves_sender_on_every_line() {
        let body = format!("hello\n\x1bworld café 🐈 {}", "x".repeat(400));
        let lines = tell_lines("bryce", &body);
        assert!(lines.len() > 2);
        assert!(lines.iter().all(|l| l.len() <= DISPLAY_BYTES
            && l.starts_with("bryce>> ")
            && l.bytes().all(|b| (32..=126).contains(&b))));
        assert!(lines[0].contains("caf? ?"));
        assert_eq!(lines.iter().map(|l| l.bytes().filter(|&b| b == b'x').count()).sum::<usize>(), 400);
        assert!(tell_lines("\nFake>>", "text")[0].starts_with("_Fake__>> "));
        assert_eq!(tell_lines("", "text"), ["Unknown>> text"]);
    }

    #[test]
    fn display_wraps_at_words_and_handles_exact_limit() {
        let width = DISPLAY_BYTES - "bryce>> ".len();
        assert_eq!(tell_lines("bryce", &"x".repeat(width))[0].len(), DISPLAY_BYTES);
        assert_eq!(tell_lines("bryce", &"x".repeat(width + 1)).len(), 2);
        let lines = tell_lines("bryce", &format!("{} next", "x".repeat(width - 2)));
        assert_eq!(lines[1], "bryce>> next");
        assert!(tell_lines("bryce", " \n\t").is_empty());
    }

    #[test]
    fn request_preserves_unicode_and_retry_identity_without_debug_leak() {
        let text = "Private café 🐈\nsecond line".to_string();
        let request = PendingMessage::new("bryce", text.clone(), false, [0xab; 16]).unwrap();
        let retry = request.clone();
        assert_eq!(request.body, text);
        assert_eq!(retry.client_id, "ab".repeat(16));
        assert_eq!(retry.client_id, request.client_id);
        assert_eq!(retry.body, request.body);
        assert!(!format!("{request:?}").contains("Private"));
        assert!(!format!("{request:?}").contains("bryce"));
        let _ = tell_lines(&request.to, &request.body);
        assert_eq!(request.body, text);
    }

    #[test]
    fn limits_count_characters_not_display_bytes() {
        assert!(PendingMessage::new("bryce", "é".repeat(1000), false, [0; 16]).is_ok());
        assert!(matches!(PendingMessage::new("bryce", "é".repeat(1001), false, [0; 16]), Err(InvalidMessage::TooLong)));
        assert!(PendingMessage::new("bot", "é".repeat(200), true, [0; 16]).is_ok());
        assert!(matches!(PendingMessage::new("bot", "é".repeat(201), true, [0; 16]), Err(InvalidMessage::TooLong)));
        assert!(matches!(PendingMessage::new("bryce", " \n".into(), false, [0; 16]), Err(InvalidMessage::Empty)));
        assert!(matches!(
            PendingMessage::new("\nbryce", "hello".into(), false, [0; 16]),
            Err(InvalidMessage::Recipient)
        ));
    }
}

use super::friends_feed::FeedCred;
use serde::Deserialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_secs(3);
const BACKOFF: Duration = Duration::from_secs(30);
const MAX_RESPONSE: u64 = 2 * 1024 * 1024;

#[derive(Clone, Deserialize)]
pub struct Message {
    pub id: u64,
    #[serde(rename = "from")]
    pub sender: String,
    pub body: String,
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Message { <redacted> }")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Denied,
    RateLimited,
    Network,
    Invalid,
    Service,
}
impl Error {
    pub fn text(self) -> &'static str {
        match self {
            Self::Unavailable => "Banshee chat is unavailable. Keep the launcher signed in.",
            Self::Denied => "Banshee sign-in expired. Waiting for the launcher to refresh it.",
            Self::RateLimited => "Please wait a minute, then retry.",
            Self::Network => "Could not confirm the request. Retry when connected.",
            Self::Invalid => "The request could not be accepted. Check the friend and message.",
            Self::Service => "Banshee chat is temporarily unavailable. Please retry.",
        }
    }
}

#[derive(Deserialize)]
struct Inbox {
    messages: Vec<Message>,
    cursor: u64,
}
#[derive(Deserialize)]
struct Conversation {
    messages: Vec<Message>,
}

#[derive(Default)]
struct InboxCursor(Option<u64>);
impl InboxCursor {
    fn accept(&mut self, page: Inbox) -> Result<Vec<Message>, Error> {
        if page.cursor > i64::MAX as u64 || page.messages.len() > 50 {
            return Err(Error::Service);
        }
        let Some(previous) = self.0 else {
            self.0 = Some(page.cursor);
            return Ok(Vec::new()); // The bootstrap never replays old mail, even from a malformed service.
        };
        if page.cursor < previous {
            return Err(Error::Service);
        }
        let mut last = previous;
        for m in &page.messages {
            if m.id <= last || m.id > page.cursor || m.body.chars().count() > MESSAGE_CHARACTERS || m.sender.len() > 128
            {
                return Err(Error::Service);
            }
            last = m.id;
        }
        self.0 = Some(page.cursor);
        Ok(page.messages)
    }
}

/// Derive only sibling API paths on the launcher's HTTPS origin. No redirect can carry the token elsewhere.
fn api_base(url: &str) -> Option<String> {
    let uri: ureq::http::Uri = url.parse().ok()?;
    if uri.scheme_str() != Some("https") || uri.query().is_some() {
        return None;
    }
    let authority = uri.authority()?.as_str();
    if authority.contains('@') {
        return None;
    }
    let prefix = uri.path().strip_suffix("/v1/social/friends")?;
    Some(format!("https://{authority}{prefix}/v1/social"))
}

fn query_name(name: &str) -> String {
    name.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn request(agent: &ureq::Agent, url: &str, token: &str, body: Option<&PendingMessage>) -> Result<Vec<u8>, Error> {
    let mut auth = ureq::http::HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| Error::Unavailable)?;
    auth.set_sensitive(true);
    let response = if let Some(m) = body {
        let bytes = serde_json::to_vec(&serde_json::json!({"to":m.to,"body":m.body,"client_id":m.client_id}))
            .map_err(|_| Error::Invalid)?;
        agent
            .post(url)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .send(bytes.as_slice())
    } else {
        agent.get(url).header("Authorization", auth).header("Accept", "application/json").call()
    };
    let mut response = response.map_err(|_| Error::Network)?;
    match response.status().as_u16() {
        200..=299 => {}
        401 | 403 => return Err(Error::Denied),
        429 => return Err(Error::RateLimited),
        400 | 404 | 409 | 413 | 422 => return Err(Error::Invalid),
        _ => return Err(Error::Service),
    }
    response.body_mut().with_config().limit(MAX_RESPONSE).read_to_vec().map_err(|_| Error::Service)
}

pub enum Event {
    Available(bool),
    Incoming(Vec<Message>),
    Sent { client_id: String, result: Result<(), Error> },
    History { request: u64, result: Result<Vec<Message>, Error> },
}
enum Command {
    Send(PendingMessage),
    History { peer: String, after: u64, request: u64 },
}

pub struct Social {
    commands: mpsc::SyncSender<Command>,
    pub events: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
}
impl Drop for Social {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl Social {
    pub fn send(&self, message: PendingMessage) -> bool {
        self.commands.try_send(Command::Send(message)).is_ok()
    }
    pub fn history(&self, peer: String, after: u64, request: u64) -> bool {
        self.commands.try_send(Command::History { peer, after, request }).is_ok()
    }
}

// Do not repeat a denied token after zoning/reconnecting either. The launcher alone mints/replaces tokens.
static REJECTED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
fn denied(token: &str) -> bool {
    REJECTED.lock().unwrap_or_else(|e| e.into_inner()).as_deref() == Some(token)
}
fn reject(token: &str) {
    *REJECTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.to_owned());
}

pub fn spawn(cred: FeedCred) -> Social {
    let base = api_base(&cred.url);
    spawn_at(cred, base)
}

fn spawn_at(cred: FeedCred, base: Option<String>) -> Social {
    let (commands, rx) = mpsc::sync_channel(8);
    let (tx, events) = mpsc::sync_channel(8);
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    std::thread::spawn(move || {
        let Some(base) = base else {
            let _ = tx.send(Event::Available(false));
            return;
        };
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(concat!("vana-client/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let mut cursor = InboxCursor::default();
        let mut next_poll = Instant::now();
        let mut next_token = Instant::now();
        let mut token: Option<String> = None;
        let mut available = None;
        while !stopping.load(Ordering::Relaxed) {
            if Instant::now() >= next_token {
                token = cred.current_token().filter(|(t, s)| s == "social.chat" && !denied(t)).map(|t| t.0);
                next_token = Instant::now() + POLL;
            }
            let ready = token.is_some();
            if available != Some(ready) {
                if tx.send(Event::Available(ready)).is_err() {
                    return;
                }
                available = Some(ready);
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(command) => {
                    // Read scope/token again for every user action, including a rollback to social.read.
                    let current = cred.current_token().filter(|(t, s)| s == "social.chat" && !denied(t)).map(|t| t.0);
                    token = current.clone();
                    let get = |path: &str, body| {
                        current
                            .as_ref()
                            .ok_or(Error::Unavailable)
                            .and_then(|t| request(&agent, &format!("{base}/{path}"), t, body))
                    };
                    let (event, error) = match command {
                        Command::Send(m) => {
                            let result = get("messages", Some(&m)).and_then(|bytes| {
                                let v: serde_json::Value =
                                    serde_json::from_slice(&bytes).map_err(|_| Error::Service)?;
                                v.get("id")
                                    .and_then(|v| v.as_u64())
                                    .filter(|id| *id > 0)
                                    .map(|_| ())
                                    .ok_or(Error::Service)
                            });
                            let error = result.as_ref().err().copied();
                            (Event::Sent { client_id: m.client_id, result }, error)
                        }
                        Command::History { peer, after, request: id } => {
                            let result = get(&format!("messages?with={}&after={after}", query_name(&peer)), None)
                                .and_then(|bytes| {
                                    let page: Conversation =
                                        serde_json::from_slice(&bytes).map_err(|_| Error::Service)?;
                                    let mut last = after;
                                    if page.messages.len() > 200 {
                                        return Err(Error::Service);
                                    }
                                    for m in &page.messages {
                                        if m.id <= last
                                            || m.body.chars().count() > MESSAGE_CHARACTERS
                                            || m.sender.len() > 128
                                        {
                                            return Err(Error::Service);
                                        }
                                        last = m.id;
                                    }
                                    Ok(page.messages)
                                });
                            let error = result.as_ref().err().copied();
                            (Event::History { request: id, result }, error)
                        }
                    };
                    if error == Some(Error::Denied) {
                        if let Some(t) = &current {
                            reject(t);
                        }
                        token = None;
                    }
                    if tx.send(event).is_err() {
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if stopping.load(Ordering::Relaxed) {
                return;
            }
            if cred.inbox_tells
                && Instant::now() >= next_poll
                && let Some(t) = &token
            {
                let path = cursor.0.map_or_else(|| format!("{base}/inbox"), |id| format!("{base}/inbox?after={id}"));
                let result = request(&agent, &path, t, None)
                    .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|_| Error::Service))
                    .and_then(|page| cursor.accept(page));
                next_poll = Instant::now() + if result.is_ok() { POLL } else { BACKOFF };
                match result {
                    Ok(messages) if !messages.is_empty() => {
                        if tx.send(Event::Incoming(messages)).is_err() {
                            return;
                        }
                    }
                    Err(Error::Denied) => {
                        reject(t);
                        token = None;
                    }
                    _ => {} // No body, peer, token or response text is ever logged.
                }
            }
        }
    });
    Social { commands, events, stop }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    #[test]
    fn social_inbox_bootstrap_dedupe_and_cursor_validation() {
        let message = |id| Message { id, sender: "alice".into(), body: "private fixture".into() };
        let mut c = InboxCursor::default();
        assert!(c.accept(Inbox { messages: vec![message(3)], cursor: 3 }).unwrap().is_empty());
        assert_eq!(c.accept(Inbox { messages: vec![message(4)], cursor: 4 }).unwrap().len(), 1);
        assert!(c.accept(Inbox { messages: vec![message(4)], cursor: 4 }).is_err());
        assert_eq!(c.0, Some(4));
        assert!(c.accept(Inbox { messages: vec![], cursor: 2 }).is_err());
        assert!(c.accept(Inbox { messages: vec![], cursor: 4 }).unwrap().is_empty());
    }
    #[test]
    fn social_endpoints_keep_origin_and_encode_peers() {
        assert_eq!(
            api_base("https://accounts.example/v1/social/friends").as_deref(),
            Some("https://accounts.example/v1/social")
        );
        for url in [
            "http://accounts.example/v1/social/friends",
            "https://u@accounts.example/v1/social/friends",
            "https://accounts.example/elsewhere",
            "https://accounts.example/v1/social/friends?next=x",
        ] {
            assert!(api_base(url).is_none());
        }
        assert_eq!(query_name("a&with=b"), "a%26with%3Db");
        assert!(POLL >= Duration::from_secs(3));
        assert!(
            !format!("{:?}", Message { id: 1, sender: "private peer".into(), body: "private body".into() })
                .contains("private")
        );
    }
}

#[cfg(test)]
mod social_wire_tests {
    use super::*;
    use std::io::{Read, Write};
    fn server(replies: Vec<(u16, String)>) -> (String, std::thread::JoinHandle<Vec<(String, Vec<u8>)>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let job = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, reply) in replies {
                let deadline = Instant::now() + Duration::from_secs(12);
                let mut stream = loop {
                    if let Ok((s, _)) = listener.accept() {
                        break s;
                    }
                    assert!(Instant::now() < deadline, "fixture request timeout");
                    std::thread::sleep(Duration::from_millis(10));
                };
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut data = Vec::new();
                let mut byte = [0];
                while !data.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    data.push(byte[0]);
                }
                let headers = String::from_utf8(data).unwrap();
                let count = headers
                    .lines()
                    .find_map(|s| {
                        s.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|n| n.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; count];
                stream.read_exact(&mut body).unwrap();
                requests.push((headers, body));
                write!(stream,"HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",reply.len()).unwrap();
            }
            requests
        });
        (url, job)
    }
    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(3)))
            .max_redirects(0)
            .http_status_as_error(false)
            .proxy(None)
            .build()
            .into()
    }
    #[test]
    fn social_wire_send_retry_preserves_unicode_identity_and_maps_errors() {
        let (url, job) = server(vec![
            (503, "{}".into()),
            (200, r#"{"id":19}"#.into()),
            (429, "{}".into()),
            (401, "{}".into()),
            (400, "{}".into()),
            (302, "{}".into()),
        ]);
        let message = PendingMessage::new("alice", "private café fixture".into(), false, [0x12; 16]).unwrap();
        let a = agent();
        assert_eq!(request(&a, &url, "fixture-token", Some(&message)), Err(Error::Service));
        assert!(request(&a, &url, "fixture-token", Some(&message)).is_ok());
        for expected in [Error::RateLimited, Error::Denied, Error::Invalid, Error::Service] {
            assert_eq!(request(&a, &url, "fixture-token", None), Err(expected));
        }
        let requests = job.join().unwrap();
        assert_eq!(requests[0].1, requests[1].1);
        let v: serde_json::Value = serde_json::from_slice(&requests[0].1).unwrap();
        assert_eq!(v["body"], message.body);
        assert_eq!(v["client_id"], message.client_id);
        assert_eq!(v["to"], "alice");
        assert!(requests[0].0.starts_with("POST / HTTP/1.1"));
        assert!(requests[0].0.to_ascii_lowercase().contains("authorization: bearer fixture-token"));
    }
    #[test]
    fn social_worker_bootstraps_then_delivers_only_new_mail() {
        let (url, job) = server(vec![
            (200, r#"{"messages":[{"id":4,"from":"alice","body":"old fixture"}],"cursor":4}"#.into()),
            (200, r#"{"messages":[{"id":5,"from":"alice","body":"new fixture"}],"cursor":5}"#.into()),
        ]);
        let mut cred = FeedCred::new("https://example.invalid/v1/social/friends", "inbox-fixture-token").unwrap();
        cred.scope = "social.chat".into();
        cred.inbox_tells = true;
        let s = spawn_at(cred, Some(url));
        assert!(matches!(s.events.recv_timeout(Duration::from_secs(2)).unwrap(), Event::Available(true)));
        let start = Instant::now();
        let Event::Incoming(messages) = s.events.recv_timeout(Duration::from_secs(8)).unwrap() else {
            panic!("expected new mail")
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, 5);
        assert!(start.elapsed() >= Duration::from_secs(3));
        drop(s);
        let requests = job.join().unwrap();
        assert!(requests[0].0.starts_with("GET /inbox HTTP/1.1"));
        assert!(requests[1].0.starts_with("GET /inbox?after=4 HTTP/1.1"));
    }
    #[test]
    fn social_worker_inbox_disabled_still_sends_and_reads_conversation() {
        let (url, job) = server(vec![
            (200, r#"{"id":8,"message":"delivered (ignored)"}"#.into()),
            (200, r#"{"messages":[{"id":8,"from":"alice","body":"fixture"}]}"#.into()),
        ]);
        let mut cred = FeedCred::new("https://example.invalid/v1/social/friends", "send-fixture-token").unwrap();
        cred.scope = "social.chat".into();
        assert!(!cred.inbox_tells);
        let s = spawn_at(cred, Some(url));
        assert!(matches!(s.events.recv_timeout(Duration::from_secs(2)).unwrap(), Event::Available(true)));
        assert!(s.send(PendingMessage::new("alice", "fixture".into(), false, [1; 16]).unwrap()));
        assert!(matches!(s.events.recv_timeout(Duration::from_secs(2)).unwrap(), Event::Sent { result: Ok(()), .. }));
        assert!(s.history("alice".into(), 0, 7));
        assert!(matches!(
            s.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::History { request: 7, result: Ok(_), .. }
        ));
        assert!(s.events.recv_timeout(Duration::from_millis(3200)).is_err());
        drop(s);
        let requests = job.join().unwrap();
        assert!(requests[0].0.starts_with("POST /messages HTTP/1.1"));
        assert!(requests[1].0.starts_with("GET /messages?with=alice&after=0 HTTP/1.1"));
    }
}

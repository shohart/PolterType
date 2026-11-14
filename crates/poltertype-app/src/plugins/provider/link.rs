//! `ProviderLink` — one open channel to one text provider.

use std::io::{BufReader, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use parking_lot::Mutex;
use poltertype_core::plugins::TextProviderManifest;
use poltertype_core::text_provider::{
    LineError, MAX_LINE_BYTES, RawReply, Reply, Request, SOCKET_ENV, TOKEN_ENV, check_reply,
    encode_request, is_hello, parse_reply,
};
use poltertype_ipc::{Channel, Endpoint, random_token};
use tracing::{info, warn};

use super::consts::{CONNECT_WITHIN, HELLO_WITHIN, REPLY_QUEUE, REQUEST_QUEUE};
use super::lines::{Line, read_bounded_line};
use super::types::{ProviderEnv, Shared};

/// A channel PolterType listens on for one provider's service.
///
/// Opened before the service is spawned, so the environment the service
/// starts with can name it. Until the service connects and proves itself
/// every request is no opinion; after it dies, the same. Dropping the
/// link closes the channel, which is how consent is withdrawn.
///
/// Nothing a provider sends is logged, and nothing PolterType sends
/// either: a malformed reply is reported once per session, by kind.
pub struct ProviderLink {
    id: String,
    manifest: TextProviderManifest,
    requests: Sender<String>,
    replies: Mutex<Receiver<(u64, RawReply)>>,
    next_id: AtomicU64,
    shared: Arc<Shared>,
}

impl ProviderLink {
    /// Listen for plug-in `id` and return the link with the environment
    /// its service must be started with. `Err` where the platform has no
    /// transport, or the socket could not be made.
    pub fn open(
        id: &str,
        manifest: &TextProviderManifest,
    ) -> std::io::Result<(Arc<Self>, ProviderEnv)> {
        let endpoint = Endpoint::bind()?;
        let token = random_token()?;
        let env = vec![
            (SOCKET_ENV, endpoint.path().as_os_str().to_owned()),
            (TOKEN_ENV, token.clone().into()),
        ];
        let (req_tx, req_rx) = bounded::<String>(REQUEST_QUEUE);
        let (rep_tx, rep_rx) = bounded::<(u64, RawReply)>(REPLY_QUEUE);
        let shared = Arc::new(Shared {
            connected: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            close: Mutex::new(None),
            reported_malformed: AtomicBool::new(false),
        });

        let link = Arc::new(Self {
            id: id.to_owned(),
            manifest: manifest.clone(),
            requests: req_tx,
            replies: Mutex::new(rep_rx),
            next_id: AtomicU64::new(1),
            shared: Arc::clone(&shared),
        });

        let plugin = id.to_owned();
        std::thread::Builder::new()
            .name(format!("pt-provider-{id}"))
            .spawn(move || serve(&plugin, endpoint, &token, &shared, &req_rx, &rep_tx))?;
        Ok((link, env))
    }

    /// The plug-in this link talks to.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Whether the service has connected and proved itself, and the
    /// channel is still open.
    pub fn is_connected(&self) -> bool {
        self.shared.connected.load(Ordering::Acquire) && !self.shared.closed.load(Ordering::Acquire)
    }

    /// What the manifest says this provider answers.
    pub fn manifest(&self) -> &TextProviderManifest {
        &self.manifest
    }

    /// Ask, and wait no longer than the manifest's deadline for this kind
    /// of request. Anything but a timely reply that passes every check is
    /// [`Reply::Pass`]: not connected, queue full, late, malformed.
    pub fn request(&self, request: &Request) -> Reply {
        let deadline = match request {
            Request::Suggest { .. } => self.manifest.suggest_deadline_ms,
            Request::Rewrite { .. } => self.manifest.rewrite_deadline_ms,
        };
        self.request_within(request, Duration::from_millis(deadline))
    }

    fn request_within(&self, request: &Request, within: Duration) -> Reply {
        if !self.is_connected() {
            return Reply::Pass;
        }
        let until = Instant::now() + within;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        // One caller at a time: replies are matched by id, and a second
        // caller would only steal the first one's answer.
        let replies = self.replies.lock();
        // Replies to requests already given up on.
        while replies.try_recv().is_ok() {}
        if self.requests.try_send(encode_request(id, request)).is_err() {
            return Reply::Pass;
        }
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match replies.recv_timeout(left) {
                Ok((got, raw)) if got == id => return check_reply(request, raw),
                // Late, for a request this link gave up on: dropped.
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    return Reply::Pass;
                }
            }
        }
    }

    /// Close the channel. The provider sees it end; requests from now on
    /// are no opinion.
    pub fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
        if let Some(close) = self.shared.close.lock().take() {
            close();
        }
    }
}

impl Drop for ProviderLink {
    fn drop(&mut self) {
        self.close();
    }
}

/// The link's thread: wait for the service, then read its replies until
/// the channel ends. Writing has a thread of its own, so a provider that
/// stops reading can never block a reply being read, or the caller.
fn serve(
    id: &str,
    endpoint: Endpoint,
    token: &str,
    shared: &Arc<Shared>,
    requests: &Receiver<String>,
    replies: &Sender<(u64, RawReply)>,
) {
    let accepted = endpoint.accept(
        Instant::now() + CONNECT_WITHIN,
        HELLO_WITHIN,
        &|line| is_hello(line, token),
        &|| shared.closed.load(Ordering::Acquire),
    );
    // One client only: the name goes away as soon as it has been used.
    drop(endpoint);
    let Channel {
        reader,
        mut writer,
        close,
    } = match accepted {
        Ok(c) => c,
        Err(e) => {
            if !shared.closed.load(Ordering::Acquire) {
                warn!(plugin = id, "text provider never connected: {e}");
            }
            return;
        }
    };
    *shared.close.lock() = Some(close);
    // Closed while the hello was being read: undo what was just set up.
    if shared.closed.load(Ordering::Acquire) {
        if let Some(close) = shared.close.lock().take() {
            close();
        }
        return;
    }
    shared.connected.store(true, Ordering::Release);
    info!(plugin = id, "text provider connected");

    let outgoing = requests.clone();
    let writer_shared = Arc::clone(shared);
    let _ = std::thread::Builder::new()
        .name(format!("pt-provider-{id}-w"))
        .spawn(move || {
            for line in outgoing {
                if writer
                    .write_all(line.as_bytes())
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    writer_shared.connected.store(false, Ordering::Release);
                    return;
                }
            }
        });

    let mut reader = BufReader::new(reader);
    loop {
        match read_bounded_line(&mut reader, MAX_LINE_BYTES) {
            Line::Text(line) if line.trim().is_empty() => {}
            Line::Text(line) => match parse_reply(&line) {
                // Full: nobody is waiting for these, so losing one is
                // losing nothing.
                Ok(reply) => {
                    let _ = replies.try_send(reply);
                }
                Err(LineError::Malformed) => {
                    if !shared.reported_malformed.swap(true, Ordering::AcqRel) {
                        warn!(
                            plugin = id,
                            "text provider sent a line that is not a reply; ignoring it and any more"
                        );
                    }
                }
                Err(LineError::TooLong) => {
                    warn!(plugin = id, "text provider sent an overlong line; closing");
                    break;
                }
            },
            Line::TooLong => {
                warn!(plugin = id, "text provider sent an overlong line; closing");
                break;
            }
            Line::End => break,
        }
    }
    shared.connected.store(false, Ordering::Release);
    if let Some(close) = shared.close.lock().take() {
        close();
    }
    info!(plugin = id, "text provider channel closed");
}

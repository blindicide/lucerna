//! A JSON-lines client for mpv's `--input-ipc-server` socket.
//!
//! Requests carry a `request_id` and are matched to replies; lines with an `event` field are
//! delivered on a separate channel. Requests are written in the order [`MpvIpc::send`] is called.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep, timeout};

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("could not connect to the mpv control socket {path}: {source}")]
    Connect {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("the connection to mpv was closed")]
    Closed,
    #[error("mpv did not answer in time")]
    Timeout,
    /// mpv answered with an error status such as `property unavailable`.
    #[error("mpv rejected the command: {0}")]
    Command(String),
}

/// The events Lucerna cares about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MpvEvent {
    FileLoaded,
    EndFile {
        reason: Option<String>,
        file_error: Option<String>,
    },
    Shutdown,
    Other(String),
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

pub struct MpvIpc {
    writer: mpsc::UnboundedSender<String>,
    pending: Pending,
    next_id: AtomicU64,
}

/// How long a single request may wait for its reply.
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);

impl MpvIpc {
    /// Connect, retrying every `interval` until `deadline` has elapsed (mpv creates the socket a
    /// moment after it starts). Returns the client and the event stream; the stream ends when the
    /// connection closes.
    pub async fn connect(
        path: &Path,
        interval: Duration,
        deadline: Duration,
        give_up: impl Fn() -> bool,
    ) -> Result<(Self, mpsc::UnboundedReceiver<MpvEvent>), IpcError> {
        let started = Instant::now();
        let stream = loop {
            match UnixStream::connect(path).await {
                Ok(stream) => break stream,
                Err(source) => {
                    if give_up() || started.elapsed() >= deadline {
                        return Err(IpcError::Connect {
                            path: path.to_path_buf(),
                            source,
                        });
                    }
                    sleep(interval).await;
                }
            }
        };
        Ok(Self::from_stream(stream))
    }

    fn from_stream(stream: UnixStream) -> (Self, mpsc::UnboundedReceiver<MpvEvent>) {
        let (read_half, mut write_half) = stream.into_split();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (writer_tx, mut writer_rx) = mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            while let Some(line) = writer_rx.recv().await {
                if write_half.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        let reader_pending = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut lines = BufReader::new(read_half).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if let Some(event) = value.get("event").and_then(Value::as_str) {
                    let _ = event_tx.send(to_event(event, &value));
                } else if let Some(id) = value.get("request_id").and_then(Value::as_u64) {
                    let waiter = reader_pending.lock().ok().and_then(|mut p| p.remove(&id));
                    if let Some(waiter) = waiter {
                        let _ = waiter.send(value);
                    }
                }
            }
            // Connection closed: fail everything still waiting; dropping event_tx ends the stream.
            if let Ok(mut pending) = reader_pending.lock() {
                pending.clear();
            }
        });

        (
            Self {
                writer: writer_tx,
                pending,
                next_id: AtomicU64::new(1),
            },
            event_rx,
        )
    }

    /// Queue a command immediately and return the receiver for its reply. Commands are written to
    /// the socket in call order.
    pub fn send(&self, args: &[Value]) -> Result<oneshot::Receiver<Value>, IpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| IpcError::Closed)?
            .insert(id, tx);
        let mut line = json!({ "command": args, "request_id": id }).to_string();
        line.push('\n');
        if self.writer.send(line).is_err() {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&id);
            }
            return Err(IpcError::Closed);
        }
        Ok(rx)
    }

    /// Send a command and wait for its result (`data`, or `Null`).
    pub async fn command(&self, args: &[Value]) -> Result<Value, IpcError> {
        let rx = self.send(args)?;
        match timeout(REPLY_TIMEOUT, rx).await {
            Err(_) => Err(IpcError::Timeout),
            Ok(Err(_)) => Err(IpcError::Closed),
            Ok(Ok(reply)) => match reply.get("error").and_then(Value::as_str) {
                Some("success") | None => Ok(reply.get("data").cloned().unwrap_or(Value::Null)),
                Some(other) => Err(IpcError::Command(other.to_owned())),
            },
        }
    }

    pub async fn set_property(&self, name: &str, value: impl Into<Value>) -> Result<(), IpcError> {
        self.command(&[json!("set_property"), json!(name), value.into()])
            .await
            .map(|_| ())
    }

    pub async fn get_property(&self, name: &str) -> Result<Value, IpcError> {
        self.command(&[json!("get_property"), json!(name)]).await
    }
}

fn to_event(name: &str, value: &Value) -> MpvEvent {
    match name {
        "file-loaded" => MpvEvent::FileLoaded,
        "shutdown" => MpvEvent::Shutdown,
        "end-file" => MpvEvent::EndFile {
            reason: value
                .get("reason")
                .and_then(Value::as_str)
                .map(str::to_owned),
            file_error: value
                .get("file_error")
                .and_then(Value::as_str)
                .map(str::to_owned),
        },
        other => MpvEvent::Other(other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::UnixListener;

    fn socket_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lucerna-ipc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("s.sock")
    }

    async fn connect(path: &Path) -> (MpvIpc, mpsc::UnboundedReceiver<MpvEvent>) {
        MpvIpc::connect(
            path,
            Duration::from_millis(10),
            Duration::from_secs(2),
            || false,
        )
        .await
        .unwrap()
    }

    #[test]
    fn event_mapping() {
        let v = json!({"event": "end-file", "reason": "error", "file_error": "unrecognized file format"});
        assert_eq!(
            to_event("end-file", &v),
            MpvEvent::EndFile {
                reason: Some("error".into()),
                file_error: Some("unrecognized file format".into())
            }
        );
        assert_eq!(to_event("file-loaded", &json!({})), MpvEvent::FileLoaded);
        assert_eq!(to_event("seek", &json!({})), MpvEvent::Other("seek".into()));
    }

    #[tokio::test]
    async fn requests_are_correlated_and_events_are_delivered() {
        let path = socket_path("corr");
        let listener = UnixListener::bind(&path).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (r, mut w) = stream.into_split();
            let mut lines = BufReader::new(r).lines();
            w.write_all(b"{\"event\":\"file-loaded\"}\n").await.unwrap();
            // Answer out of order to prove correlation by request_id.
            let first = lines.next_line().await.unwrap().unwrap();
            let second = lines.next_line().await.unwrap().unwrap();
            for line in [second, first] {
                let v: Value = serde_json::from_str(&line).unwrap();
                let id = v["request_id"].as_u64().unwrap();
                let reply = json!({"error": "success", "data": format!("re:{}", v["command"][1]), "request_id": id});
                w.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
            }
        });

        let (ipc, mut events) = connect(&path).await;
        assert_eq!(events.recv().await, Some(MpvEvent::FileLoaded));
        let a = ipc.send(&[json!("get_property"), json!("a")]).unwrap();
        let b = ipc.send(&[json!("get_property"), json!("b")]).unwrap();
        assert_eq!(b.await.unwrap()["data"], "re:\"b\"");
        assert_eq!(a.await.unwrap()["data"], "re:\"a\"");
    }

    #[tokio::test]
    async fn error_status_becomes_a_command_error() {
        let path = socket_path("err");
        let listener = UnixListener::bind(&path).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (r, mut w) = stream.into_split();
            let mut lines = BufReader::new(r).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let v: Value = serde_json::from_str(&line).unwrap();
                let reply = json!({"error": "property unavailable", "request_id": v["request_id"]});
                w.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
            }
        });
        let (ipc, _events) = connect(&path).await;
        match ipc.get_property("time-pos").await {
            Err(IpcError::Command(msg)) => assert_eq!(msg, "property unavailable"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn closing_the_socket_ends_the_event_stream_and_fails_pending_requests() {
        let path = socket_path("close");
        let listener = UnixListener::bind(&path).unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 64];
            let _ = stream.read(&mut buf).await; // wait for the request, then hang up
        });
        let (ipc, mut events) = connect(&path).await;
        let result = ipc.command(&[json!("get_property"), json!("x")]).await;
        assert!(matches!(result, Err(IpcError::Closed)), "{result:?}");
        assert_eq!(events.recv().await, None);
    }

    #[tokio::test]
    async fn connecting_to_a_missing_socket_times_out_with_context() {
        let path = socket_path("missing");
        let err = MpvIpc::connect(
            &path,
            Duration::from_millis(10),
            Duration::from_millis(60),
            || false,
        )
        .await
        .err()
        .unwrap();
        assert!(matches!(err, IpcError::Connect { .. }));
        assert!(err.to_string().contains("s.sock"));
    }

    #[tokio::test]
    async fn give_up_predicate_stops_connecting_early() {
        let path = socket_path("giveup");
        let started = std::time::Instant::now();
        let err = MpvIpc::connect(
            &path,
            Duration::from_millis(10),
            Duration::from_secs(30),
            || true,
        )
        .await
        .err();
        assert!(err.is_some());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

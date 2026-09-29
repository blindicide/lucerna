//! Bounded capture of mpv's stdout and stderr (directive §9).
//!
//! Lines are truncated, kept in a small in-memory ring for diagnostics, and written to a
//! size-bounded rotating log file. They are *not* forwarded to `tracing` one by one, so a noisy
//! renderer cannot flood the journal.

use std::collections::VecDeque;
use std::sync::Mutex;

use lucerna_core::bounded_log::BoundedLog;
use tokio::io::{AsyncRead, AsyncReadExt};

const MAX_LINE: usize = 4096;
const RING_LINES: usize = 200;

pub struct Capture {
    ring: Mutex<VecDeque<String>>,
    log: Mutex<Option<BoundedLog>>,
}

impl Capture {
    pub fn new(log: Option<BoundedLog>) -> Self {
        Self {
            ring: Mutex::new(VecDeque::new()),
            log: Mutex::new(log),
        }
    }

    fn push(&self, stream: &str, line: String) {
        if let Ok(mut log) = self.log.lock()
            && let Some(log) = log.as_mut()
        {
            let _ = log.write_line(&format!("[{stream}] {line}"));
        }
        if stream == "stderr"
            && let Ok(mut ring) = self.ring.lock()
        {
            if ring.len() == RING_LINES {
                ring.pop_front();
            }
            ring.push_back(line);
        }
    }

    /// The last `n` stderr lines.
    pub fn tail(&self, n: usize) -> Vec<String> {
        self.ring
            .lock()
            .map(|ring| ring.iter().rev().take(n).rev().cloned().collect())
            .unwrap_or_default()
    }

    /// Number of stderr lines currently retained (bounded by the ring size).
    #[cfg(test)]
    pub fn retained(&self) -> usize {
        self.ring.lock().map(|r| r.len()).unwrap_or(0)
    }

    /// Read `reader` to EOF, splitting lines and truncating each to [`MAX_LINE`] bytes.
    pub async fn pump<R: AsyncRead + Unpin>(&self, mut reader: R, stream: &'static str) {
        let mut chunk = [0u8; 8192];
        let mut line: Vec<u8> = Vec::with_capacity(256);
        let mut truncated = false;
        loop {
            let n = match reader.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for &byte in &chunk[..n] {
                if byte == b'\n' {
                    self.push(stream, finish_line(&mut line, truncated));
                    truncated = false;
                } else if line.len() < MAX_LINE {
                    line.push(byte);
                } else {
                    truncated = true;
                }
            }
        }
        if !line.is_empty() {
            self.push(stream, finish_line(&mut line, truncated));
        }
    }
}

fn finish_line(line: &mut Vec<u8>, truncated: bool) -> String {
    let mut text = String::from_utf8_lossy(line)
        .trim_end_matches('\r')
        .to_owned();
    if truncated {
        text.push_str(" [truncated]");
    }
    line.clear();
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ring_keeps_only_the_last_lines() {
        let capture = Capture::new(None);
        let data: String = (0..1000).map(|i| format!("line {i}\n")).collect();
        capture.pump(data.as_bytes(), "stderr").await;
        assert_eq!(capture.retained(), RING_LINES);
        assert_eq!(
            capture.tail(2),
            vec!["line 998".to_owned(), "line 999".to_owned()]
        );
    }

    #[tokio::test]
    async fn long_lines_are_truncated_and_stdout_is_not_retained() {
        let capture = Capture::new(None);
        let data = format!("{}\n", "z".repeat(100_000));
        capture.pump(data.as_bytes(), "stderr").await;
        capture.pump("out\n".as_bytes(), "stdout").await;
        let tail = capture.tail(5);
        assert_eq!(tail.len(), 1);
        assert!(tail[0].len() < MAX_LINE + 20 && tail[0].ends_with("[truncated]"));
    }

    #[tokio::test]
    async fn last_line_without_newline_is_kept() {
        let capture = Capture::new(None);
        capture.pump("no newline".as_bytes(), "stderr").await;
        assert_eq!(capture.tail(1), vec!["no newline".to_owned()]);
    }
}

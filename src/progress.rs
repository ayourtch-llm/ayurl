use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::io::AsyncRead;
use pin_project_lite::pin_project;

/// Progress information for an ongoing transfer.
#[derive(Debug, Clone)]
pub struct Progress {
    pub bytes_transferred: u64,
    pub total_bytes: Option<u64>,
    pub elapsed: Duration,
}

/// Type-erased progress callback.
pub type ProgressCallback = Arc<dyn Fn(&Progress) + Send + Sync>;

/// A sink that receives progress updates — either a callback or a watch channel.
pub enum ProgressSink {
    Callback(ProgressCallback),
    Channel(tokio::sync::watch::Sender<Progress>),
}

impl ProgressSink {
    fn report(&self, progress: &Progress) {
        match self {
            ProgressSink::Callback(cb) => cb(progress),
            ProgressSink::Channel(tx) => {
                let _ = tx.send(progress.clone());
            }
        }
    }
}

pin_project! {
    /// Wraps an `AsyncRead` and tracks bytes read, firing progress updates.
    pub struct ProgressReader<R> {
        #[pin]
        inner: R,
        bytes_transferred: u64,
        total_bytes: Option<u64>,
        start: Instant,
        sink: ProgressSink,
    }
}

impl<R> ProgressReader<R> {
    pub fn new(inner: R, total_bytes: Option<u64>, sink: ProgressSink) -> Self {
        Self {
            inner,
            bytes_transferred: 0,
            total_bytes,
            start: Instant::now(),
            sink,
        }
    }

}

impl<R: AsyncRead> AsyncRead for ProgressReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        match this.inner.poll_read(cx, buf) {
            Poll::Ready(Ok(n)) => {
                *this.bytes_transferred += n as u64;
                let progress = Progress {
                    bytes_transferred: *this.bytes_transferred,
                    total_bytes: *this.total_bytes,
                    elapsed: this.start.elapsed(),
                };
                this.sink.report(&progress);
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }
}

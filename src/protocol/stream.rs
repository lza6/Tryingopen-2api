//! 辅助：reqwest bytes stream → tokio AsyncRead（SSE 逐行读取）

use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};

pub fn reader_with_bytes<S>(stream: S) -> BytesReader<S>
where
    S: futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin + Send + 'static,
{
    BytesReader { stream, buf: Vec::new(), eof: false }
}

pub struct BytesReader<S> {
    stream: S,
    buf: Vec<u8>,
    eof: bool,
}

impl<S> AsyncRead for BytesReader<S>
where
    S: futures::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin + Send,
{
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        loop {
            if !self.buf.is_empty() {
                let n = std::cmp::min(buf.remaining(), self.buf.len());
                buf.put_slice(&self.buf[..n]);
                self.buf.drain(..n);
                return Poll::Ready(Ok(()));
            }
            if self.eof { return Poll::Ready(Ok(())); }
            match Pin::new(&mut self.stream).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => { self.eof = true; return Poll::Ready(Ok(())); }
                Poll::Ready(Some(Err(_))) => { self.eof = true; return Poll::Ready(Ok(())); }
                Poll::Ready(Some(Ok(chunk))) => self.buf.extend_from_slice(&chunk),
            }
        }
    }
}

//! Transport-neutral peer stream. The sync protocol (length-prefixed JSON) runs
//! unchanged over either a LAN TCP connection or an iroh QUIC stream (internet,
//! NAT hole-punching with relay fallback).
//!
//! Reads go through a small internal buffer so `wait_readable` can wait for
//! data without consuming a partial message — the same cancel-safety the TCP
//! path previously got from `peek`, which QUIC streams don't have.

use std::io;
use std::net::IpAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub enum Transport {
    Tcp(TcpStream),
    Iroh {
        send: iroh::endpoint::SendStream,
        recv: iroh::endpoint::RecvStream,
        // Keeps the QUIC connection open for as long as the stream lives.
        _conn: iroh::endpoint::Connection,
    },
}

pub struct PeerStream {
    inner: Transport,
    buf: Vec<u8>,
    pos: usize,
}

impl PeerStream {
    pub fn tcp(stream: TcpStream) -> Self {
        crate::network::protocol::configure_keepalive(&stream);
        Self::from_transport(Transport::Tcp(stream))
    }

    pub fn iroh(
        conn: iroh::endpoint::Connection,
        send: iroh::endpoint::SendStream,
        recv: iroh::endpoint::RecvStream,
    ) -> Self {
        Self::from_transport(Transport::Iroh { send, recv, _conn: conn })
    }

    fn from_transport(inner: Transport) -> Self {
        Self { inner, buf: Vec::new(), pos: 0 }
    }

    /// "LAN" for TCP, "Internet" for iroh.
    pub fn kind(&self) -> &'static str {
        match self.inner {
            Transport::Tcp(_) => "LAN",
            Transport::Iroh { .. } => "Internet",
        }
    }

    /// The peer's iroh endpoint id — authenticated by the QUIC handshake, so
    /// unlike a node id claimed in Hello it can be trusted. None for TCP.
    pub fn remote_node_id(&self) -> Option<iroh::EndpointId> {
        match &self.inner {
            Transport::Tcp(_) => None,
            Transport::Iroh { _conn, .. } => Some(_conn.remote_id()),
        }
    }

    /// Remote IP for TCP peers (iroh peers are identified by key, not address).
    pub fn peer_ip(&self) -> Option<IpAddr> {
        match &self.inner {
            Transport::Tcp(s) => s.peer_addr().ok().map(|a| a.ip()),
            Transport::Iroh { .. } => None,
        }
    }

    async fn read_inner(&mut self, out: &mut [u8]) -> io::Result<usize> {
        match &mut self.inner {
            Transport::Tcp(s) => s.read(out).await,
            Transport::Iroh { recv, .. } => AsyncReadExt::read(recv, out).await,
        }
    }

    /// Wait up to `timeout` for incoming data without consuming it.
    /// `Ok(Some(true))` = data available, `Ok(Some(false))` = peer closed,
    /// `Ok(None)` = nothing arrived. Cancel-safe: a single `read` either
    /// completes (bytes go into the internal buffer) or does nothing.
    pub async fn wait_readable(&mut self, timeout: Duration) -> io::Result<Option<bool>> {
        if self.pos < self.buf.len() {
            return Ok(Some(true));
        }
        let mut tmp = [0u8; 16 * 1024];
        match tokio::time::timeout(timeout, self.read_inner(&mut tmp)).await {
            Err(_) => Ok(None),
            Ok(Ok(0)) => Ok(Some(false)),
            Ok(Ok(n)) => {
                self.buf.clear();
                self.pos = 0;
                self.buf.extend_from_slice(&tmp[..n]);
                Ok(Some(true))
            }
            Ok(Err(e)) => Err(e),
        }
    }

    pub async fn read_exact(&mut self, out: &mut [u8]) -> io::Result<()> {
        let mut filled = 0;
        // Serve buffered bytes first.
        if self.pos < self.buf.len() {
            let n = (self.buf.len() - self.pos).min(out.len());
            out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            filled = n;
            if self.pos == self.buf.len() {
                self.buf.clear();
                self.pos = 0;
            }
        }
        if filled < out.len() {
            match &mut self.inner {
                Transport::Tcp(s) => {
                    s.read_exact(&mut out[filled..]).await?;
                }
                Transport::Iroh { recv, .. } => {
                    AsyncReadExt::read_exact(recv, &mut out[filled..]).await?;
                }
            }
        }
        Ok(())
    }

    pub async fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match &mut self.inner {
            Transport::Tcp(s) => s.write_all(data).await,
            Transport::Iroh { send, .. } => AsyncWriteExt::write_all(send, data).await,
        }
    }

    pub async fn flush(&mut self) -> io::Result<()> {
        match &mut self.inner {
            Transport::Tcp(s) => s.flush().await,
            Transport::Iroh { send, .. } => AsyncWriteExt::flush(send).await,
        }
    }

    /// After a final message (a refusal), make sure it's delivered before the
    /// connection is dropped. Dropping an iroh connection right after writing
    /// discarded the data, so a client refused for a wrong PIN or game over
    /// the internet saw only "connection lost" instead of the PIN prompt or
    /// the switch-game offer.
    pub async fn close_gracefully(&mut self) {
        match &mut self.inner {
            Transport::Tcp(s) => {
                let _ = s.flush().await;
                let _ = AsyncWriteExt::shutdown(s).await;
            }
            Transport::Iroh { send, .. } => {
                let _ = send.finish();
                let _ = tokio::time::timeout(Duration::from_secs(3), send.stopped()).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wait_readable_does_not_lose_bytes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::spawn(async move { TcpStream::connect(addr).await.unwrap() });
        let (server, _) = listener.accept().await.unwrap();
        let mut writer = PeerStream::tcp(client.await.unwrap());
        let mut reader = PeerStream::tcp(server);

        // Nothing sent yet: times out without error.
        assert!(reader.wait_readable(Duration::from_millis(50)).await.unwrap().is_none());

        writer.write_all(b"hello world").await.unwrap();
        writer.flush().await.unwrap();
        assert_eq!(reader.wait_readable(Duration::from_secs(2)).await.unwrap(), Some(true));

        // Buffered bytes plus the rest are read in order.
        let mut out = [0u8; 11];
        reader.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"hello world");

        drop(writer);
        assert_eq!(reader.wait_readable(Duration::from_secs(2)).await.unwrap(), Some(false));
    }

    /// The real sync protocol over an iroh QUIC stream between two local
    /// endpoints (relays disabled, so no network access is needed).
    #[tokio::test]
    async fn protocol_messages_over_iroh() {
        use crate::network::protocol::{recv_message, send_message, try_recv_message, Message};
        use iroh::{endpoint::presets, Endpoint, RelayMode};
        const ALPN: &[u8] = b"synccrate-test/1";

        let host = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let client = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .bind()
            .await
            .unwrap();
        let host_addr = host.addr();

        let server = tokio::spawn(async move {
            let conn = host.accept().await.unwrap().await.unwrap();
            let (send, recv) = conn.accept_bi().await.unwrap();
            let mut s = PeerStream::iroh(conn, send, recv);
            assert_eq!(s.kind(), "Internet");
            match recv_message(&mut s).await.unwrap() {
                Message::Hello { name, .. } => assert_eq!(name, "Alice"),
                other => panic!("unexpected {:?}", other),
            }
            send_message(&mut s, &Message::Welcome { name: "Host".into(), version: "t".into(), supports_compression: true, game_id: None, node_id: None, crews: vec![], features: vec![] })
                .await
                .unwrap();
            // Keep the connection open until the client has read the reply.
            let _ = tokio::time::timeout(Duration::from_secs(5), s.wait_readable(Duration::from_secs(5))).await;
        });

        let conn = client.connect(host_addr, ALPN).await.unwrap();
        let (send, recv) = conn.open_bi().await.unwrap();
        let mut c = PeerStream::iroh(conn, send, recv);
        send_message(&mut c, &Message::Hello { name: "Alice".into(), version: "t".into(), pin: None, supports_compression: true, game_id: None, node_id: None, crews: vec![], features: vec![] })
            .await
            .unwrap();
        match try_recv_message(&mut c, Duration::from_secs(10)).await.unwrap() {
            Some(Message::Welcome { name, .. }) => assert_eq!(name, "Host"),
            other => panic!("unexpected {:?}", other),
        }
        drop(c);
        server.await.unwrap();
    }
}

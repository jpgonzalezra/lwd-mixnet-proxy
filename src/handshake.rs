//! The header both halves exchange before a single wallet byte moves.
//!
//! It exists to reproduce, on purpose and cheaply, the failure this transport has without it: a
//! stream opens, the far side accepts it, and the first payload never arrives, with neither end
//! erroring or timing out. Sending a header and waiting for it back puts that failure under a
//! deadline the dialler controls, so a dead stream can be discarded before it is handed anything
//! that matters.
//!
//! The payload is not lost. It reaches the far side before the `Open` that registers its stream,
//! and the pinned SDK discards frames for streams it has not seen. Fixed on the SDK's `develop`
//! and in no release, so the header stays until a release carries it.
//!
//! ```text
//! byte  0..4   magic, "LWMP"
//! byte  4      protocol version
//! byte  5      flags; bit 0 asks the listener to echo the header back
//! byte  6..14  token, echoed verbatim
//! ```

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Identifies our own framing. A stream that opens with anything else is dropped before it can
/// reach an upstream, so the listener is not an open relay for arbitrary mixnet traffic.
const MAGIC: [u8; 4] = *b"LWMP";

const VERSION: u8 = 1;

/// Set when the dialler waits for the header to come back before using the stream.
const FLAG_ECHO: u8 = 0b0000_0001;

/// Wire length of the header.
pub const HEADER_LEN: usize = 14;

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

/// What one half sends the other to open a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Whether the dialler is waiting for this header to come back.
    pub echo_requested: bool,
    /// Distinguishes this header from any other, so an echo is known to carry our own bytes.
    pub token: u64,
}

impl Header {
    /// Build a header with a token no other header in this process carries.
    pub fn next(echo_requested: bool) -> Self {
        Self {
            echo_requested,
            token: NEXT_TOKEN.fetch_add(1, Ordering::Relaxed),
        }
    }

    fn encode(&self) -> [u8; HEADER_LEN] {
        let mut bytes = [0u8; HEADER_LEN];
        bytes[0..4].copy_from_slice(&MAGIC);
        bytes[4] = VERSION;
        bytes[5] = if self.echo_requested { FLAG_ECHO } else { 0 };
        bytes[6..14].copy_from_slice(&self.token.to_be_bytes());
        bytes
    }

    fn decode(bytes: &[u8; HEADER_LEN]) -> Result<Self, HandshakeError> {
        if bytes[0..4] != MAGIC {
            return Err(HandshakeError::ForeignStream);
        }
        if bytes[4] != VERSION {
            return Err(HandshakeError::UnsupportedVersion(bytes[4]));
        }
        let mut token = [0u8; 8];
        token.copy_from_slice(&bytes[6..14]);
        Ok(Self {
            echo_requested: bytes[5] & FLAG_ECHO != 0,
            token: u64::from_be_bytes(token),
        })
    }
}

/// Why a stream never became usable.
#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error("nothing came back within {0:?}")]
    TimedOut(Duration),
    #[error("reading the header: {0}")]
    Read(#[source] io::Error),
    #[error("writing the header: {0}")]
    Write(#[source] io::Error),
    #[error("the peer does not speak this protocol")]
    ForeignStream,
    #[error("the peer speaks protocol version {0}, this build speaks {VERSION}")]
    UnsupportedVersion(u8),
    #[error("the echo carried token {returned}, expected {sent}")]
    TokenMismatch { sent: u64, returned: u64 },
}

/// Send a header and wait for it back, giving up after `timeout`.
///
/// Returns how long the round trip took, which is what the probe adds to establishing a connection.
pub async fn probe<S>(stream: &mut S, timeout: Duration) -> Result<Duration, HandshakeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let started = tokio::time::Instant::now();
    let sent = Header::next(true);
    write_header(stream, sent).await?;

    let echoed = tokio::time::timeout(timeout, read_header(stream))
        .await
        .map_err(|_| HandshakeError::TimedOut(timeout))??;

    if echoed.token != sent.token {
        return Err(HandshakeError::TokenMismatch {
            sent: sent.token,
            returned: echoed.token,
        });
    }
    Ok(started.elapsed())
}

/// Send a header without waiting for anything back.
///
/// The listener still needs it to recognise the stream, so the header is not optional even when the
/// probe is switched off; only the round trip is.
pub async fn announce<S>(stream: &mut S) -> Result<(), HandshakeError>
where
    S: AsyncWrite + Unpin,
{
    write_header(stream, Header::next(false)).await
}

/// Read the dialler's header, echoing it back when asked, and give up after `timeout`.
///
/// The timeout is what keeps a stream whose payload never arrived from holding resources until the
/// SDK's own half-hour sweep collects it.
pub async fn accept<S>(stream: &mut S, timeout: Duration) -> Result<Header, HandshakeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let header = tokio::time::timeout(timeout, read_header(stream))
        .await
        .map_err(|_| HandshakeError::TimedOut(timeout))??;

    if header.echo_requested {
        write_header(stream, header).await?;
    }
    Ok(header)
}

async fn write_header<S>(stream: &mut S, header: Header) -> Result<(), HandshakeError>
where
    S: AsyncWrite + Unpin,
{
    stream
        .write_all(&header.encode())
        .await
        .map_err(HandshakeError::Write)?;
    stream.flush().await.map_err(HandshakeError::Write)
}

async fn read_header<S>(stream: &mut S) -> Result<Header, HandshakeError>
where
    S: AsyncRead + Unpin,
{
    let mut bytes = [0u8; HEADER_LEN];
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(HandshakeError::Read)?;
    Header::decode(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_survives_a_round_trip_through_the_wire_format() {
        let header = Header {
            echo_requested: true,
            token: 0x0123_4567_89ab_cdef,
        };
        assert_eq!(Header::decode(&header.encode()).unwrap(), header);
    }

    #[test]
    fn every_header_carries_its_own_token() {
        assert_ne!(Header::next(true).token, Header::next(true).token);
    }

    #[test]
    fn a_stream_that_opens_with_foreign_bytes_is_rejected() {
        let error = Header::decode(&[0u8; HEADER_LEN]).unwrap_err();
        assert!(matches!(error, HandshakeError::ForeignStream));
    }

    #[test]
    fn a_header_from_a_future_version_is_rejected() {
        let mut bytes = Header::next(false).encode();
        bytes[4] = VERSION + 1;
        let error = Header::decode(&bytes).unwrap_err();
        assert!(
            matches!(error, HandshakeError::UnsupportedVersion(version) if version == VERSION + 1)
        );
    }

    #[tokio::test]
    async fn a_probe_that_is_echoed_succeeds() {
        let (mut dialler, mut listener) = tokio::io::duplex(64);
        let (probed, accepted) = tokio::join!(
            probe(&mut dialler, Duration::from_secs(5)),
            accept(&mut listener, Duration::from_secs(5))
        );
        assert!(probed.is_ok() && accepted.unwrap().echo_requested);
    }

    #[tokio::test(start_paused = true)]
    async fn a_probe_the_peer_never_answers_times_out() {
        let (mut dialler, _listener) = tokio::io::duplex(64);
        let error = probe(&mut dialler, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(error, HandshakeError::TimedOut(_)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_listener_the_dialler_never_writes_to_times_out() {
        let (_dialler, mut listener) = tokio::io::duplex(64);
        let error = accept(&mut listener, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(error, HandshakeError::TimedOut(_)));
    }

    #[tokio::test]
    async fn an_announced_stream_is_not_echoed() {
        let (mut dialler, mut listener) = tokio::io::duplex(64);
        let (announced, accepted) = tokio::join!(
            announce(&mut dialler),
            accept(&mut listener, Duration::from_secs(5))
        );
        announced.unwrap();
        assert!(!accepted.unwrap().echo_requested);
    }

    #[tokio::test]
    async fn an_echo_carrying_another_token_is_rejected() {
        let (mut dialler, mut listener) = tokio::io::duplex(64);
        let impostor = async {
            let mut bytes = [0u8; HEADER_LEN];
            listener.read_exact(&mut bytes).await.unwrap();
            let mut reply = Header::next(true);
            reply.token = u64::MAX;
            listener.write_all(&reply.encode()).await.unwrap();
        };
        let (probed, ()) = tokio::join!(probe(&mut dialler, Duration::from_secs(5)), impostor);
        assert!(matches!(
            probed.unwrap_err(),
            HandshakeError::TokenMismatch { .. }
        ));
    }
}

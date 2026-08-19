//! Carrying a light-wallet gRPC connection over a mixnet, transparently to both ends.
//!
//! Two processes, each a byte pipe, with the mixnet between them:
//!
//! ```text
//! wallet --TCP--> [lwd-mixnet-client] --mixnet--> [lwd-mixnet-server] --TCP--> lightwalletd
//! ```
//!
//! Neither half understands gRPC. A mixnet stream implements `AsyncRead + AsyncWrite`, so gRPC
//! travels through unmodified and neither the wallet nor the server needs to know any of this is
//! happening.
//!
//! What the halves do add is a deadline. The transport loses a stream's first payload often, and
//! silently: both ends hang instead of erroring, and silence is not something a gRPC library can act
//! on. So a stream is [probed](handshake) before the wallet is allowed near it, discarded and
//! [replaced](dial) if it does not answer, and [torn down](splice) if it stops answering later.

pub mod dial;
pub mod endpoint;
pub mod handshake;
pub mod health;
pub mod metrics;
pub mod shutdown;
pub mod splice;
pub mod streaks;

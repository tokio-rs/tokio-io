//! Utilities for encoding and decoding frames.
//!
//! Contains adapters to go from strams of bytes, [`AsyncRead`] and
//! [`AsyncWrite`], to framed streams implementing [`Sink`] and [`Stream`].
//! Framed streams are also known as [transports].
//!
//! [`AsyncRead`]: #
//! [`AsyncWrite`]: #
//! [`Sink`]: #
//! [`Stream`]: #
//! [transports]: #

pub use framed::Framed;
pub use framed_read::{FramedRead, Decoder};
pub use framed_write::{FramedWrite, Encoder};

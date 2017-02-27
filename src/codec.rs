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

pub use frame::{EasyBuf, EasyBufMut, Framed, Codec};

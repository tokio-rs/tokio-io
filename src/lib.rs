//! Core I/O traits and combinators when working with Tokio.
//!
//! A description of the high-level I/O combinators can be [found online] in
//! addition to a description of the [low level details].
//!
//! [found online]: https://tokio.rs/docs/getting-started/core/
//! [low level details]: https://tokio.rs/docs/going-deeper/core-low-level/

#![deny(missing_docs)]
#![doc(html_root_url = "https://docs.rs/tokio-io/0.1")]

#[macro_use]
extern crate log;
extern crate futures;

use std::io as std_io;

use futures::{BoxFuture, Async};
use futures::stream::BoxStream;

/// A convenience typedef around a `Future` whose error component is `io::Error`
pub type IoFuture<T> = BoxFuture<T, std_io::Error>;

/// A convenience typedef around a `Stream` whose error component is `io::Error`
pub type IoStream<T> = BoxStream<T, std_io::Error>;

/// A convenience macro for working with `io::Result<T>` from the `Read` and
/// `Write` traits.
///
/// This macro takes `io::Result<T>` as input, and returns `T` as the output. If
/// the input type is of the `Err` variant, then `Poll::NotReady` is returned if
/// it indicates `WouldBlock` or otherwise `Err` is returned.
#[macro_export]
macro_rules! try_nb {
    ($e:expr) => (match $e {
        Ok(t) => t,
        Err(ref e) if e.kind() == ::std::io::ErrorKind::WouldBlock => {
            return Ok(::futures::Async::NotReady)
        }
        Err(e) => return Err(e.into()),
    })
}

pub mod io;
pub mod codec;

mod copy;
mod flush;
mod frame;
mod lines;
mod read;
mod read_exact;
mod read_to_end;
mod read_until;
mod split;
mod window;
mod write_all;

use frame::{Codec, Framed};
use split::{ReadHalf, WriteHalf};

/// A trait for readable objects which operated in an asynchronous and
/// futures-aware fashion.
///
/// This trait inherits from `io::Read` and indicates as a marker that an I/O
/// object is **nonblocking**, meaning that it will return an error instead of
/// blocking when bytes are read. Specifically this means that the `read`
/// function for traits that implement this type can have a few return values:
///
/// * `Ok(n)` means that `n` bytes of data was immediately read and placed into
///   the output buffer, where `n` == 0 implies that EOF has been reached.
/// * `Err(e) if e.kind() == ErrorKind::WouldBlock` means that no data was read
///   into the buffer provided. The I/O object is not currently readable but may
///   become readable in the future. Most importantly, **the current future's
///   task is scheduled to get unparked when the object is readable**. This
///   means that like `Future::poll` you'll receive a notification when the I/O
///   object is readable again.
/// * `Err(e)` for other errors are standard I/O errors coming from the
///   underlying object.
///
/// This trait importantly means that the `read` method only works in the
/// context of a future's task. The object may panic if used outside of a task.
pub trait AsyncRead: std_io::Read {
    /// Tests to see if this I/O object may be readable.
    ///
    /// This method returns an `Async<()>` indicating whether the object
    /// **might** be readable. It is possible that even if this method returns
    /// `Async::Ready` that a call to `read` would return a `WouldBlock` error.
    ///
    /// There is a default implementation for this function which always
    /// indicates that an I/O object is readable, but objects which can
    /// implement a finer grained version of this are recommended to do so.
    ///
    /// If this function returns `Async::NotReady` then the current future's
    /// task is arranged to receive a notification when it might not return
    /// `NotReady`.
    ///
    /// # Panics
    ///
    /// This method is likely to panic if called from outside the context of a
    /// future's task.
    fn poll_read(&mut self) -> Async<()> {
        Async::Ready(())
    }

    /// Provides a `Stream` and `Sink` interface for reading and writing to this
    /// `Io` object, using `Decode` and `Encode` to read and write the raw data.
    ///
    /// Raw I/O objects work with byte sequences, but higher-level code usually
    /// wants to batch these into meaningful chunks, called "frames". This
    /// method layers framing on top of an I/O object, by using the `Codec`
    /// traits to handle encoding and decoding of messages frames. Note that
    /// the incoming and outgoing frame types may be distinct.
    ///
    /// This function returns a *single* object that is both `Stream` and
    /// `Sink`; grouping this into a single object is often useful for layering
    /// things like gzip or TLS, which require both read and write access to the
    /// underlying object.
    ///
    /// If you want to work more directly with the streams and sink, consider
    /// calling `split` on the `Framed` returned by this method, which will
    /// break them into separate objects, allowing them to interact more easily.
    fn framed<C: Codec>(self, codec: C) -> Framed<Self, C>
        where Self: AsyncWrite + Sized,
    {
        frame::framed(self, codec)
    }

    /// Helper method for splitting this read/write object into two halves.
    ///
    /// The two halves returned implement the `Read` and `Write` traits,
    /// respectively.
    fn split(self) -> (ReadHalf<Self>, WriteHalf<Self>)
        where Self: AsyncWrite + Sized,
    {
        split::split(self)
    }
}

impl<T: ?Sized + AsyncRead> AsyncRead for Box<T> {
    fn poll_read(&mut self) -> Async<()> {
        (**self).poll_read()
    }
}
impl<'a, T: ?Sized + AsyncRead> AsyncRead for &'a mut T {
    fn poll_read(&mut self) -> Async<()> {
        (**self).poll_read()
    }
}

/// A trait for writable objects which operated in an asynchronous and
/// futures-aware fashion.
///
/// This trait inherits from `io::Write` and indicates as a marker that an I/O
/// object is **nonblocking**, meaning that it will return an error instead of
/// blocking when bytes are written. Specifically this means that the `write`
/// function for traits that implement this type can have a few return values:
///
/// * `Ok(n)` means that `n` bytes of data was immediately written .
/// * `Err(e) if e.kind() == ErrorKind::WouldBlock` means that no data was
///   written from the buffer provided. The I/O object is not currently
///   writable but may become writable in the future. Most importantly, **the
///   current future's task is scheduled to get unparked when the object is
///   readable**. This means that like `Future::poll` you'll receive a
///   notification when the I/O object is writable again.
/// * `Err(e)` for other errors are standard I/O errors coming from the
///   underlying object.
///
/// This trait importantly means that the `write` method only works in the
/// context of a future's task. The object may panic if used outside of a task.
pub trait AsyncWrite: std_io::Write {
    /// Tests to see if this I/O object may be writable.
    ///
    /// This method returns an `Async<()>` indicating whether the object
    /// **might** be writable. It is possible that even if this method returns
    /// `Async::Ready` that a call to `write` would return a `WouldBlock` error.
    ///
    /// There is a default implementation for this function which always
    /// indicates that an I/O object is writable, but objects which can
    /// implement a finer grained version of this are recommended to do so.
    ///
    /// If this function returns `Async::NotReady` then the current future's
    /// task is arranged to receive a notification when it might not return
    /// `NotReady`.
    ///
    /// # Panics
    ///
    /// This method is likely to panic if called from outside the context of a
    /// future's task.
    fn poll_write(&mut self) -> Async<()> {
        Async::Ready(())
    }
}

impl<T: ?Sized + AsyncWrite> AsyncWrite for Box<T> {
    fn poll_write(&mut self) -> Async<()> {
        (**self).poll_write()
    }
}
impl<'a, T: ?Sized + AsyncWrite> AsyncWrite for &'a mut T {
    fn poll_write(&mut self) -> Async<()> {
        (**self).poll_write()
    }
}

impl AsyncRead for std_io::Repeat {}
impl AsyncWrite for std_io::Sink {}
impl<T: AsyncRead> AsyncRead for std_io::Take<T> {}

use std::io::{self, Read, Write};
use std::fmt;

use {AsyncRead, AsyncWrite};
use framed_read::{framed_read2, FramedRead2, Decoder};
use framed_write::{framed_write2, FramedWrite2, Encoder};

use futures::{Stream, Sink, StartSend, Poll};
use bytes::{BytesMut};

/// A unified `Stream` and `Sink` interface to an underlying I/O object, using
/// the `Encoder` and `Decoder` traits to encode and decode frames.
///
/// You can create a `Framed` instance by using the `AsyncRead::framed` adapter.
pub struct Framed<T, U> {
    inner: FramedRead2<FramedWrite2<Fuse<T, U>>>,
}

pub struct Fuse<T, U>(pub T, pub U);

pub fn framed<T, U>(inner: T, codec: U) -> Framed<T, U>
    where T: AsyncRead + AsyncWrite,
          U: Decoder + Encoder,
{
    Framed {
        inner: framed_read2(framed_write2(Fuse(inner, codec))),
    }
}

impl<T, U> Framed<T, U> {
    /// Returns a reference to the underlying I/O stream wrapped by
    /// `Frame`.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise
    /// being worked with.
    pub fn get_ref(&self) -> &T {
        &self.inner.get_ref().get_ref().0
    }

    /// Returns a mutable reference to the underlying I/O stream wrapped by
    /// `Frame`.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise
    /// being worked with.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner.get_mut().get_mut().0
    }

    /// Consumes the `Frame`, returning its underlying I/O stream.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise
    /// being worked with.
    pub fn into_inner(self) -> T {
        self.inner.into_inner().into_inner().0
    }
}

impl<T, U> Stream for Framed<T, U>
    where T: AsyncRead,
          U: Decoder,
{
    type Item = U::Item;
    type Error = U::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.inner.poll()
    }
}

impl<T, U> Sink for Framed<T, U>
    where T: AsyncWrite,
          U: Encoder,
          U::Error: From<io::Error>,
{
    type SinkItem = U::Item;
    type SinkError = U::Error;

    fn start_send(&mut self,
                  item: Self::SinkItem)
                  -> StartSend<Self::SinkItem, Self::SinkError>
    {
        self.inner.get_mut().start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.get_mut().poll_complete()
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.get_mut().close()
    }
}

impl<T, U> fmt::Debug for Framed<T, U>
    where T: fmt::Debug,
          U: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Framed")
         .field("io", &self.inner.get_ref().get_ref().0)
         .field("codec", &self.inner.get_ref().get_ref().1)
         .finish()
    }
}

// ===== impl Fuse =====

impl<T: Read, U> Read for Fuse<T, U> {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        self.0.read(dst)
    }
}

impl<T: AsyncRead, U> AsyncRead for Fuse<T, U> {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.0.prepare_uninitialized_buffer(buf)
    }
}

impl<T: Write, U> Write for Fuse<T, U> {
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        self.0.write(src)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<T: AsyncWrite, U> AsyncWrite for Fuse<T, U> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.0.shutdown()
    }
}

impl<T, U: Decoder> Decoder for Fuse<T, U> {
    type Item = U::Item;
    type Error = U::Error;

    fn decode(&mut self, buffer: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.1.decode(buffer)
    }

    fn decode_eof(&mut self, buffer: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.1.decode_eof(buffer)
    }
}

impl<T, U: Encoder> Encoder for Fuse<T, U> {
    type Item = U::Item;
    type Error = U::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.1.encode(item, dst)
    }
}

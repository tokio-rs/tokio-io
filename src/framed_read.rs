use AsyncRead;
use framed::Fuse;

use futures::{Async, Poll, Stream, Sink, StartSend};
use bytes::BytesMut;

use std::io;

/// Decoding of frames via buffers.
///
/// This trait is used when constructing an instance of `Framed` or
/// `FramedRead`. An implementation of `Decoder` takes a byte stream that has
/// already been buffered in `src` and decodes the data into a stream of
/// `Self::Item` frames.
///
/// Implementations are able to track state on `self`, which enables
/// implementing stateful streaming parsers. In many cases, though, this type
/// will simply be a unit struct (e.g. `struct HttpDecoder`).
pub trait Decoder {
    /// The type of decoded frames.
    type Item;

    /// Attempts to decode a message from the provided buffer of bytes.
    ///
    /// This method is called by `FramedRead` whenever new data becomes
    /// available. If a complete message is available, its constituent bytes
    /// should be consumed (for example, with `BytesMut::drain_to`) and
    /// Ok(Some(message)) returned.
    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<Self::Item>>;

    /// A method that can optionally be overridden to handle EOF specially.
    ///
    /// This method will never be provided with bytes that have not previously
    /// been provided to `decode`.
    fn decode_eof(&mut self, buf: &mut BytesMut) -> io::Result<Self::Item> {
        match try!(self.decode(buf)) {
            Some(frame) => Ok(frame),
            None => Err(io::Error::new(io::ErrorKind::Other,
                                       "bytes remaining on stream")),
        }
    }
}

/// A `Stream` of messages decoded from an `AsyncRead`.
pub struct FramedRead<T, D> {
    inner: FramedRead2<Fuse<T, D>>,
}

pub struct FramedRead2<T> {
    inner: T,
    eof: bool,
    is_readable: bool,
    buffer: BytesMut,
}

const INITIAL_CAPACITY: usize = 8 * 1024;

// ===== impl FramedRead =====

impl<T, D> FramedRead<T, D>
    where T: AsyncRead,
          D: Decoder,
{
    /// Creates a new `FramedRead` with the given `decoder`.
    pub fn new(inner: T, decoder: D) -> FramedRead<T, D> {
        FramedRead {
            inner: framed_read2(Fuse(inner, decoder)),
        }
    }

    /// Returns a reference to the underlying I/O stream wrapped by
    /// `FramedRead`.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise
    /// being worked with.
    pub fn get_ref(&self) -> &T {
        &self.inner.inner.0
    }

    /// Returns a mutable reference to the underlying I/O stream wrapped by
    /// `FramedRead`.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise
    /// being worked with.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner.inner.0
    }

    /// Consumes the `FramedRead`, returning its underlying I/O stream.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise
    /// being worked with.
    pub fn into_inner(self) -> T {
        self.inner.inner.0
    }

    /// Returns a reference to the underlying decoder.
    pub fn decoder(&self) -> &D {
        &self.inner.inner.1
    }

    /// Returns a mutable reference to the underlying decoder.
    pub fn decoder_mut(&mut self) -> &mut D {
        &mut self.inner.inner.1
    }
}

impl<T, D> Stream for FramedRead<T, D>
    where T: AsyncRead,
          D: Decoder,
{
    type Item = D::Item;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, io::Error> {
        self.inner.poll()
    }
}

impl<T, D> Sink for FramedRead<T, D>
    where T: Sink,
{
    type SinkItem = T::SinkItem;
    type SinkError = T::SinkError;

    fn start_send(&mut self,
                  item: Self::SinkItem)
                  -> StartSend<Self::SinkItem, Self::SinkError>
    {
        self.inner.inner.0.start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.inner.0.poll_complete()
    }
}

// ===== impl FramedRead2 =====

pub fn framed_read2<T>(inner: T) -> FramedRead2<T> {
    FramedRead2 {
        inner: inner,
        eof: false,
        is_readable: false,
        buffer: BytesMut::with_capacity(INITIAL_CAPACITY),
    }
}

impl<T> FramedRead2<T> {
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T> Stream for FramedRead2<T>
    where T: AsyncRead + Decoder,
{
    type Item = T::Item;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            // If the read buffer has any pending data, then it could be
            // possible that `decode` will return a new frame. We leave it to
            // the decoder to optimize detecting that more data is required.
            if self.is_readable {
                if self.eof {
                    if self.buffer.is_empty() {
                        return Ok(None.into())
                    } else {
                        let frame = try!(self.inner.decode_eof(&mut self.buffer));
                        return Ok(Async::Ready(Some(frame)));
                    }
                }

                trace!("attempting to decode a frame");

                if let Some(frame) = try!(self.inner.decode(&mut self.buffer)) {
                    trace!("frame decoded from buffer");
                    return Ok(Async::Ready(Some(frame)));
                }

                self.is_readable = false;
            }

            assert!(!self.eof);

            // Otherwise, try to read more data and try again
            if 0 == try_ready!(self.inner.read_buf(&mut self.buffer)) {
                self.eof = true;
            }

            self.is_readable = true;
        }
    }
}

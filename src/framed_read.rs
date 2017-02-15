use std::io;

use futures::{Async, Poll, Stream};

use {AsyncRead, EasyBuf};

/// Trait of helper objects for decoding messages from an `AsyncRead`, for use
/// with `FramedRead`.
pub trait Decoder {
    /// The type of messages decoded from the `AsyncRead`
    type Item;

    /// The type of fatal decoding errors.
    ///
    /// Non-fatal errors should be encoded in Item values.
    type Error;

    /// Attempts to decode a message from the provided buffer of bytes.
    ///
    /// This method is called by `FramedRead` whenever new data becomes
    /// available. If a complete message is available, its constituent bytes
    /// should be consumed (for example, with `EasyBuf::drain_to`) and
    /// Ok(Some(message)) returned.
    fn decode(&mut self, buffer: &mut EasyBuf) -> Result<Option<Self::Item>, Self::Error>;

    /// A method that can optionally be overridden to handle EOF specially.
    ///
    /// This method will never be provided with bytes that have not previously
    /// been provided to `decode`.
    #[allow(unused_variables)]
    fn eof(&mut self, buffer: &mut EasyBuf) -> Result<Option<Self::Item>, Self::Error> {
        Ok(None)
    }
}

/// A `Stream` of messages decoded from an `AsyncRead`.
pub struct FramedRead<R, D> {
    read: R,
    eof: bool,
    decoder: D,
    buffer: EasyBuf,
}

const WRITE_WINDOW_SIZE: usize = 8 * 1024;

pub fn framed_read<R, D>(read: R, decoder: D) -> FramedRead<R, D> {
    FramedRead {
        read: read,
        eof: false,
        decoder: decoder,
        buffer: EasyBuf::with_capacity(WRITE_WINDOW_SIZE),
    }
}

impl<R: AsyncRead, D: Decoder> Stream for FramedRead<R, D>
    where D::Error: From<io::Error>
{
    type Item = D::Item;
    type Error = D::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.eof {
            return Ok(Async::Ready(None))
        }
        loop {
            let n = {
                let mut buffer = self.buffer.get_mut();
                let old = buffer.len();
                buffer.resize(old + WRITE_WINDOW_SIZE, 0);
                let n = try_nb!(self.read.read(&mut buffer[old..]));
                buffer.resize(old + n, 0);
                n
            };
            if n == 0 {
                self.eof = true;
                return self.decoder.eof(&mut self.buffer).map(Async::Ready);
            }
            if let x@Some(_) = self.decoder.decode(&mut self.buffer)? {
                return Ok(Async::Ready(x));
            }
        }
    }
}

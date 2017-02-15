use std::fmt;
use std::io;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use futures::{Async, Poll, Stream, Sink, StartSend, AsyncSink};

use {AsyncRead, AsyncWrite, Encoder, Decoder};

/// A reference counted buffer of bytes.
///
/// An `EasyBuf` is a representation of a byte buffer where sub-slices of it can
/// be handed out efficiently, each with a `'static` lifetime which keeps the
/// data alive. The buffer also supports mutation but may require bytes to be
/// copied to complete the operation.
#[derive(Clone)]
pub struct EasyBuf {
    buf: Arc<Vec<u8>>,
    start: usize,
    end: usize,
}

/// An RAII object returned from `get_mut` which provides mutable access to the
/// underlying `Vec<u8>`.
pub struct EasyBufMut<'a> {
    buf: &'a mut Vec<u8>,
    end: &'a mut usize,
}

impl EasyBuf {
    /// Creates a new EasyBuf with no data and the default capacity.
    pub fn new() -> EasyBuf {
        EasyBuf::with_capacity(8 * 1024)
    }

    /// Creates a new EasyBuf with `cap` capacity.
    pub fn with_capacity(cap: usize) -> EasyBuf {
        EasyBuf {
            buf: Arc::new(Vec::with_capacity(cap)),
            start: 0,
            end: 0,
        }
    }

    /// Changes the starting index of this window to the index specified.
    ///
    /// Returns the windows back to chain multiple calls to this method.
    ///
    /// # Panics
    ///
    /// This method will panic if `start` is out of bounds for the underlying
    /// slice or if it comes after the `end` configured in this window.
    fn set_start(&mut self, start: usize) -> &mut EasyBuf {
        assert!(start <= self.buf.as_ref().len());
        assert!(start <= self.end);
        self.start = start;
        self
    }

    /// Changes the end index of this window to the index specified.
    ///
    /// Returns the windows back to chain multiple calls to this method.
    ///
    /// # Panics
    ///
    /// This method will panic if `end` is out of bounds for the underlying
    /// slice or if it comes after the `end` configured in this window.
    fn set_end(&mut self, end: usize) -> &mut EasyBuf {
        assert!(end <= self.buf.len());
        assert!(self.start <= end);
        self.end = end;
        self
    }

    /// Returns the number of bytes contained in this `EasyBuf`.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns the inner contents of this `EasyBuf` as a slice.
    pub fn as_slice(&self) -> &[u8] {
        self.as_ref()
    }

    /// Splits the buffer into two at the given index.
    ///
    /// Afterwards `self` contains elements `[0, at)`, and the returned `EasyBuf`
    /// contains elements `[at, len)`.
    ///
    /// This is an O(1) operation that just increases the reference count and
    /// sets a few indexes.
    ///
    /// # Panics
    ///
    /// Panics if `at > len`
    pub fn split_off(&mut self, at: usize) -> EasyBuf {
        let mut other = EasyBuf { buf: self.buf.clone(), ..*self };
        let idx = self.start + at;
        other.set_start(idx);
        self.set_end(idx);
        return other
    }

    /// Splits the buffer into two at the given index.
    ///
    /// Afterwards `self` contains elements `[at, len)`, and the returned `EasyBuf`
    /// contains elements `[0, at)`.
    ///
    /// This is an O(1) operation that just increases the reference count and
    /// sets a few indexes.
    ///
    /// # Panics
    ///
    /// Panics if `at > len`
    pub fn drain_to(&mut self, at: usize) -> EasyBuf {
        let mut other = EasyBuf { buf: self.buf.clone(), ..*self };
        let idx = self.start + at;
        other.set_end(idx);
        self.set_start(idx);
        return other
    }

    /// Returns a mutable reference to the underlying growable buffer of bytes.
    ///
    /// If this `EasyBuf` is the only instance pointing at the underlying buffer
    /// of bytes, a direct mutable reference will be returned. Otherwise the
    /// contents of this `EasyBuf` will be reallocated in a fresh `Vec<u8>`
    /// allocation with the same capacity as this allocation, and that
    /// allocation will be returned.
    ///
    /// This operation **is not O(1)** as it may clone the entire contents of
    /// this buffer.
    ///
    /// The returned `EasyBufMut` type implement `Deref` and `DerefMut` to
    /// `Vec<u8>` can the byte buffer can be manipulated using the standard
    /// `Vec<u8>` methods.
    pub fn get_mut(&mut self) -> EasyBufMut {
        // Fast path if we can get mutable access to our own current
        // buffer.
        //
        // TODO: this should be a match or an if-let
        if Arc::get_mut(&mut self.buf).is_some() {
            let buf = Arc::get_mut(&mut self.buf).unwrap();
            buf.drain(..self.start);
            self.start = 0;
            return EasyBufMut { buf: buf, end: &mut self.end }
        }

        // If we couldn't get access above then we give ourself a new buffer
        // here.
        let mut v = Vec::with_capacity(self.buf.capacity());
        v.extend_from_slice(self.as_ref());
        self.start = 0;
        self.buf = Arc::new(v);
        EasyBufMut {
            buf: Arc::get_mut(&mut self.buf).unwrap(),
            end: &mut self.end,
        }
    }
}

impl AsRef<[u8]> for EasyBuf {
    fn as_ref(&self) -> &[u8] {
        &self.buf[self.start..self.end]
    }
}

impl<'a> Deref for EasyBufMut<'a> {
    type Target = Vec<u8>;

    fn deref(&self) -> &Vec<u8> {
        self.buf
    }
}

impl<'a> DerefMut for EasyBufMut<'a> {
    fn deref_mut(&mut self) -> &mut Vec<u8> {
        self.buf
    }
}

impl From<Vec<u8>> for EasyBuf {
    fn from(vec: Vec<u8>) -> EasyBuf {
        let end = vec.len();
        EasyBuf {
            buf: Arc::new(vec),
            start: 0,
            end: end,
        }
    }
}

impl<'a> Drop for EasyBufMut<'a> {
    fn drop(&mut self) {
        *self.end = self.buf.len();
    }
}

impl fmt::Debug for EasyBuf {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let bytes = self.as_ref();
        let len = self.len();
        if len < 10 {
            write!(formatter, "EasyBuf{{len={}/{} {:?}}}", self.len(), self.buf.len(), bytes)
        } else { // choose a more compact representation
            write!(formatter, "EasyBuf{{len={}/{} [{}, {}, {}, {}, ..., {}, {}, {}, {}]}}", self.len(), self.buf.len(), bytes[0], bytes[1], bytes[2], bytes[3], bytes[len-4], bytes[len-3], bytes[len-2], bytes[len-1])
        }
    }
}

/// A unified `Stream` and `Sink` interface to an underlying `AsyncRead + AsyncWrite` object, using
/// the `Encoder` and `Decoder` traits to encode and decode frames.
///
/// You can acquire a `Framed` instance by using the `Io::framed` adapter.
pub struct Framed<T, C> {
    upstream: T,
    codec: C,
    eof: bool,
    rd: EasyBuf,
    wr: Vec<u8>,
}

impl<T, C> Stream for Framed<T, C>
    where T: AsyncRead,
          C: Decoder<Error=io::Error>,
{
    type Item = C::Item;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<C::Item>, io::Error> {
        const WRITE_WINDOW_SIZE: usize = 8 * 1024;

        if self.eof {
            return Ok(Async::Ready(None))
        }
        loop {
            let n = {
                let mut buffer = self.rd.get_mut();
                let old = buffer.len();
                buffer.resize(old + WRITE_WINDOW_SIZE, 0);
                let n = try_nb!(self.upstream.read(&mut buffer[old..]));
                buffer.resize(old + n, 0);
                n
            };
            if n == 0 {
                self.eof = true;
                return self.codec.eof(&mut self.rd).map(Async::Ready);
            }
            if let x@Some(_) = self.codec.decode(&mut self.rd)? {
                return Ok(Async::Ready(x));
            }
        }
    }
}

impl<T, C> Sink for Framed<T, C>
    where T: AsyncWrite,
          C: Encoder,
{
    type SinkItem = C::Item;
    type SinkError = io::Error;

    fn start_send(&mut self, item: C::Item) -> StartSend<C::Item, io::Error> {
        // If the buffer is already over 8KiB, then attempt to flush it. If after flushing it's
        // *still* over 8KiB, then apply backpressure (reject the send).
        if self.wr.len() > 8 * 1024 {
            try!(self.poll_complete());
            if self.wr.len() > 8 * 1024 {
                return Ok(AsyncSink::NotReady(item));
            }
        }

        self.codec.encode(item, &mut self.wr);
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), io::Error> {
        trace!("flushing framed transport");

        while !self.wr.is_empty() {
            trace!("writing; remaining={}", self.wr.len());
            let n = try_nb!(self.upstream.write(&self.wr));
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::WriteZero,
                                          "failed to write frame to transport"));
            }
            self.wr.drain(..n);
        }

        // Try flushing the underlying IO
        try_nb!(self.upstream.flush());

        trace!("framed transport flushed");
        return Ok(Async::Ready(()));
    }
}

pub fn framed<T, C>(io: T, codec: C) -> Framed<T, C> {
    Framed {
        upstream: io,
        codec: codec,
        eof: false,
        rd: EasyBuf::new(),
        wr: Vec::with_capacity(8 * 1024),
    }
}

impl<T, C> Framed<T, C> {

    /// Returns a reference to the underlying I/O stream wrapped by `Framed`.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise being
    /// worked with.
    pub fn get_ref(&self) -> &T {
        &self.upstream
    }

    /// Returns a mutable reference to the underlying I/O stream wrapped by
    /// `Framed`.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise being
    /// worked with.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.upstream
    }

    /// Consumes the `Framed`, returning its underlying I/O stream.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise being
    /// worked with.
    pub fn into_inner(self) -> T {
        self.upstream
    }
}

#[cfg(test)]
mod tests {
    use super::EasyBuf;

    #[test]
    fn debug_empty_easybuf() {
        let buf: EasyBuf = vec![].into();
        assert_eq!("EasyBuf{len=0/0 []}", format!("{:?}", buf));
    }

    #[test]
    fn debug_small_easybuf() {
        let buf: EasyBuf = vec![1, 2, 3, 4, 5, 6].into();
        assert_eq!("EasyBuf{len=6/6 [1, 2, 3, 4, 5, 6]}", format!("{:?}", buf));
    }

    #[test]
    fn debug_small_easybuf_split() {
        let mut buf: EasyBuf = vec![1, 2, 3, 4, 5, 6].into();
        let split = buf.split_off(4);
        assert_eq!("EasyBuf{len=4/6 [1, 2, 3, 4]}", format!("{:?}", buf));
        assert_eq!("EasyBuf{len=2/6 [5, 6]}", format!("{:?}", split));
    }

    #[test]
    fn debug_large_easybuf() {
        let vec: Vec<u8> = (0u8..255u8).collect();
        let buf: EasyBuf = vec.into();
        assert_eq!("EasyBuf{len=255/255 [0, 1, 2, 3, ..., 251, 252, 253, 254]}", format!("{:?}", buf));
    }

}

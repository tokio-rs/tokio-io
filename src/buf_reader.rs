use {AsyncRead, DEFAULT_BUF_SIZE};

use bytes::BufMut;

use futures::{Async, Poll};

use std::{cmp, fmt};
use std::io::{self, BufRead, SeekFrom};

/// The `BufReader` struct adds buffering to any reader.
pub struct BufReader<R> {
    inner: R,
    buf: Box<[u8]>,
    pos: usize,
    cap: usize,
}

impl<R: io::Read> BufReader<R> {
    /// Creates a new `BufReader` with a default buffer capacity.
    pub fn new(inner: R) -> BufReader<R> {
        BufReader::with_capacity(DEFAULT_BUF_SIZE, inner)
    }

    /// Creates a new `BufReader` with the specified buffer capacity.
    pub fn with_capacity(cap: usize, inner: R) -> BufReader<R> {
        BufReader {
            inner: inner,
            buf: vec![0; cap].into_boxed_slice(),
            pos: 0,
            cap: 0,
        }
    }
}

impl<R> BufReader<R> {
    /// Gets a reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader.
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// Gets a mutable reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Unwraps this `BufReader`, returning the underlying reader.
    ///
    /// Note that any leftover data in the internal buffer is lost.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: io::Read> io::Read for BufReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // If we don't have any buffered data and we're doing a massive read
        // (larger than our internal buffer), bypass our internal buffer
        // entirely.
        if self.pos == self.cap && buf.len() >= self.buf.len() {
            return self.inner.read(buf);
        }
        let nread = {
            let mut rem = try!(self.fill_buf());
            try!(rem.read(buf))
        };
        self.consume(nread);
        Ok(nread)
    }
}

impl<R: io::Read> io::BufRead for BufReader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        // If we've reached the end of our internal buffer then we need to fetch
        // some more data from the underlying reader.
        // Branch using `>=` instead of the more correct `==`
        // to tell the compiler that the pos..cap slice is always valid.
        if self.pos >= self.cap {
            debug_assert!(self.pos == self.cap);
            self.cap = try!(self.inner.read(&mut self.buf));
            self.pos = 0;
        }
        Ok(&self.buf[self.pos..self.cap])
    }

    fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt, self.cap);
    }
}

impl<R: io::Seek> io::Seek for BufReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let result: u64;
        if let SeekFrom::Current(n) = pos {
            let remainder = (self.cap - self.pos) as i64;
            // it should be safe to assume that remainder fits within an i64 as the alternative
            // means we managed to allocate 8 exbibytes and that's absurd.
            // But it's not out of the realm of possibility for some weird underlying reader to
            // support seeking by i64::min_value() so we need to handle underflow when subtracting
            // remainder.
            if let Some(offset) = n.checked_sub(remainder) {
                result = try!(self.inner.seek(SeekFrom::Current(offset)));
            } else {
                // seek backwards by our remainder, and then by the offset
                try!(self.inner.seek(SeekFrom::Current(-remainder)));
                self.pos = self.cap; // empty the buffer
                result = try!(self.inner.seek(SeekFrom::Current(n)));
            }
        } else {
            // Seeking with Start/End doesn't care about our buffer length.
            result = try!(self.inner.seek(pos));
        }
        self.pos = self.cap; // empty the buffer
        Ok(result)
    }
}

impl<R: AsyncRead> AsyncRead for BufReader<R> {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.inner.prepare_uninitialized_buffer(buf)
    }

    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        // If we don't have any buffered data and we're doing a massive read
        // (larger than our internal buffer), bypass our internal buffer
        // entirely.
        if self.pos == self.cap && unsafe { buf.bytes_mut().len() } >= self.buf.len() {
            return self.inner.read_buf(buf);
        }

        let nread = {
            let rem = try_nb!(self.fill_buf());

            let nread = cmp::min(rem.len(), buf.remaining_mut());
            buf.put_slice(&rem[0..nread]);

            nread
        };

        self.consume(nread);
        Ok(Async::Ready(nread))
    }
}

impl<R: fmt::Debug> fmt::Debug for BufReader<R> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("BufReader")
            .field("reader", &self.inner)
            .field("buffer", &format_args!("{}/{}", self.cap - self.pos, self.buf.len()))
            .finish()
    }
}

use {AsyncWrite, DEFAULT_BUF_SIZE};

use bytes::{Buf, BufMut, BytesMut};
use futures::{Async, Poll};

use std::{cmp, fmt};
use std::io::{self, SeekFrom};

/// Wraps a writer and buffers its output.
pub struct BufWriter<W> {
    inner: W,
    buf: io::Cursor<BytesMut>,
}

impl<W: io::Write> BufWriter<W> {
    /// Creates a new `BufWriter` with a default buffer capacity.
    pub fn new(inner: W) -> BufWriter<W> {
        BufWriter::with_capacity(DEFAULT_BUF_SIZE, inner)
    }

    /// Creates a new `BufWriter` with the specified buffer capacity.
    pub fn with_capacity(cap: usize, inner: W) -> BufWriter<W> {
        BufWriter {
            inner: inner,
            buf: io::Cursor::new(BytesMut::with_capacity(cap)),
        }
    }

    fn flush_once(&mut self) -> io::Result<()> {
        if !self.buf.has_remaining() {
            return Ok(());
        }

        self.do_flush()
    }

    fn flush_all(&mut self) -> io::Result<()> {
        while self.buf.has_remaining() {
            try!(self.do_flush());
        }

        Ok(())
    }

    fn do_flush(&mut self) -> io::Result<()> {
        debug_assert!(self.buf.has_remaining());

        match try!(self.inner.write(self.buf.bytes())) {
            0 => {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "failed to
                                          write the buffered data"));
            }
            n => self.buf.advance(n),
        }

        self.compact_buf();

        Ok(())
    }

    fn compact_buf(&mut self) {
        if self.buf.position() as usize == self.buf.get_ref().len() {
            // Fully written, clear the buffer
            self.buf.set_position(0);
            self.buf.get_mut().clear();
        }
    }
}

impl<W> BufWriter<W> {
    /// Gets a reference to the underlying writer.
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    /// Gets a mutable reference to the underlying writer.
    ///
    /// It is inadvisable to directly write to the underlying writer.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Unwraps this `BufWriter`, returning the underlying writer.
    ///
    /// The buffer is written out before returning the writer.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: io::Write> io::Write for BufWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = buf.len();

        if len > self.buf.get_ref().remaining_mut() {
            // `buf` can't fit in the internal buffer, so try flushing once.
            try!(self.flush_once());
        }

        let mut rem = self.buf.get_ref().remaining_mut();

        if rem == 0 {
            // No remaining space, flush the rest
            try!(self.flush_all());
            rem = self.buf.get_ref().remaining_mut();
        }

        // If the buffer is empty and `buf` is bigger than the internal buffer,
        // write directly to the upstream
        if !self.buf.has_remaining() && len >= rem {
            return self.inner.write(buf);
        }

        rem = cmp::min(rem, buf.len());

        self.buf.get_mut().put_slice(&buf[..rem]);
        Ok(rem)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_all().and_then(|()| self.get_mut().flush())
    }
}

impl<W: AsyncWrite> AsyncWrite for BufWriter<W> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.flush_all().and_then(|()| self.get_mut().shutdown())
    }

    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        let rem_before = buf.remaining();

        // The first chunk of the buffer cannot fit in the remaining buffer
        // space, so a flush will be attempted. Now, since the upstream may
        // supported scatter I/O operations, the original buffer will be
        // included as well.
        if buf.bytes().len() > self.buf.get_ref().remaining_mut() {
            try_ready!(self.inner.write_buf(&mut (&mut self.buf).chain(&mut *buf)));
            self.compact_buf();
        }

        // Flush in a loop as long as there is no remaining internal buffer
        // space, this is because we can't write "0"
        while !self.buf.get_ref().has_remaining_mut() {
            try_ready!(self.inner.write_buf(&mut (&mut self.buf).chain(&mut *buf)));
            self.compact_buf();
        }

        // If the buffer is empty and `buf`'s first chunk is bigger than the
        // internal buffer, write directly to the upstream
        if !self.buf.has_remaining() && buf.bytes().len() > self.buf.get_ref().remaining_mut() {
            return self.inner.write_buf(buf);
        }

        // Write to the internal buffer
        // TODO: I think this may need a take
        self.buf.get_mut().put(&mut *buf);
        Ok(Async::Ready(rem_before - buf.remaining()))
    }
}

impl<W: fmt::Debug> fmt::Debug for BufWriter<W> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("BufWriter")
            .field("writer", &self.inner)
            .field("buffer", &format_args!("{}/{}", self.buf.remaining(), self.buf.get_ref().capacity()))
            .finish()
    }
}

impl<W: io::Write + io::Seek> io::Seek for BufWriter<W> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.flush_all().and_then(|_| self.get_mut().seek(pos))
    }
}

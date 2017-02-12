use std::io;

use futures::{Async, Poll, Sink, StartSend, AsyncSink};

use {AsyncWrite, AsyncRead};

const BUFFER_SIZE: usize = 8 * 1024;

/// A sink that consumes AsyncRead values, copying their contents to an
/// AsyncWrite.
pub struct ByteSink<W, R> {
    write: W,
    read: Option<R>,
    write_cursor: usize,
    read_cursor: usize,
    buffer: Box<[u8]>,
}

/// Construct a sink that consumes AsyncReads, writing their entire contents to
/// the supplied AsyncWrite.
pub fn byte_sink<W, R>(write: W) -> ByteSink<W, R> {
    ByteSink {
        write: write,
        read: None,
        write_cursor: 0,
        read_cursor: 0,
        buffer: vec![0; BUFFER_SIZE].into_boxed_slice(),
    }
}

impl<W: AsyncWrite, R: AsyncRead> Sink for ByteSink<W, R> {
    type SinkItem = R;
    type SinkError = io::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        if self.read.is_some() {
            self.poll_complete()?;
            if self.read.is_some() {
                return Ok(AsyncSink::NotReady(item))
            }
        }

        self.read = Some(item);
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        loop {
            if self.read.is_some() && self.write_cursor == 0 {
                if let Async::NotReady = self.read.as_mut().unwrap().poll_read() {
                    return Ok(Async::NotReady);
                }

                self.write_cursor = try_nb!(self.read.as_mut().unwrap().read(&mut self.buffer));
                if self.write_cursor == 0 {
                    self.read.take();
                }
            }

            if self.write_cursor != 0 {
                if let Async::NotReady = self.write.poll_write() {
                    return Ok(Async::NotReady);
                }
            }

            while self.read_cursor != self.write_cursor {
                self.read_cursor += try_nb!(self.write.write(&self.buffer[self.read_cursor..self.write_cursor]));
            }
            
            self.read_cursor = 0;
            self.write_cursor = 0;

            if self.read.is_none() {
                try_nb!(self.write.flush());
                return Ok(Async::Ready(()));
            }
        }
    }
}

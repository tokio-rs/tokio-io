use std::io;

use futures::{Async, Poll, Sink, StartSend, AsyncSink};

use AsyncWrite;

/// Trait of helper objects to write out messages as bytes, for use with
/// `FramedWrite`.
pub trait Encoder {
    /// The type of items consumed by the `Encoder`
    type Item;

    /// Encode a complete Item into a byte buffer
    fn encode<T: Extend<u8>>(&mut self, item: Self::Item, buffer: &mut T);
}

/// A Sink that uses an `Encoder` to convert its `SinkItem`s to bytes and writes out
/// those bytes.
pub struct FramedWrite<W, E> {
    encoder: E,
    write: W,
    buffer: Vec<u8>,
}

pub fn framed_write<W, E>(write: W, encoder: E) -> FramedWrite<W, E> {
    FramedWrite {
        encoder: encoder,
        write: write,
        buffer: Vec::new(),
    }
}

impl<W: AsyncWrite, E: Encoder> Sink for FramedWrite<W, E>
{
    type SinkItem = E::Item;
    type SinkError = io::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        const THRESHOLD: usize = 8 * 1024;

        // If the buffer is already over THRESHOLD, then attempt to flush it. If after flushing it's *still* over
        // THRESHOLD, then apply backpressure (reject the send).
        if self.buffer.len() > THRESHOLD {
            try!(self.poll_complete());
            if self.buffer.len() > THRESHOLD {
                return Ok(AsyncSink::NotReady(item));
            }
        }

        self.encoder.encode(item, &mut self.buffer);
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), io::Error> {
        while !self.buffer.is_empty() {
            let n = try_nb!(self.write.write(&self.buffer));
            self.buffer.drain(..n);
        }

        try_nb!(self.write.flush());

        return Ok(Async::Ready(()));
    }
}

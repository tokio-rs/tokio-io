#[macro_use]
extern crate tokio_io;
extern crate futures;
extern crate bytes;

use tokio_io::AsyncWrite;
use tokio_io::io::BufWriter;

use futures::Poll;
use futures::Async::*;
use bytes::{Buf, IntoBuf};

use std::io::{self, Write};
use std::collections::VecDeque;

macro_rules! mock {
    ($($x:expr,)*) => {{
        let mut v = VecDeque::new();
        v.extend(vec![$($x),*]);
        Mock { calls: v }
    }};
}

macro_rules! assert_would_block {
    ($x:expr) => {{
        assert_eq!(io::ErrorKind::WouldBlock, ($x).unwrap_err().kind())
    }};
}

#[test]
fn buffered_writer() {
    let inner = Vec::new();
    let mut writer = BufWriter::with_capacity(32, inner);

    assert_eq!(*writer.get_ref(), []);

    writer.write(b"hello world").unwrap();
    writer.write(b"hello world").unwrap();
    assert_eq!(*writer.get_ref(), []);

    writer.write(b"hello world").unwrap();
    assert_eq!(*writer.get_ref(), b"hello worldhello world");

    writer.get_mut().clear();

    writer.flush().unwrap();
    assert_eq!(*writer.get_ref(), b"hello world");

    writer.get_mut().clear();

    // write a big slice when buffer is empty
    writer.write(b"hello world hello world hello world").unwrap();
    assert_eq!(*writer.get_ref(), &b"hello world hello world hello world"[..]);
}

#[test]
fn buf_write_would_block() {
    let mut writer = BufWriter::with_capacity(32, mock! {
        Ok(b"hello world"[..].into()),
        Err(would_block()),
        Ok(b"hello world -- goodbye world"[..].into()),
        Ok(Flush),
    });

    assert_eq!(11, writer.write(b"hello world").unwrap());
    assert_eq!(28, writer.write(b"hello world -- goodbye world").unwrap());
    assert_would_block!(writer.write(b"2 hello world"));

    assert!(writer.flush().is_ok());

    assert_eq!(13, writer.write(b"2 hello world").unwrap());

    assert!(writer.get_ref().calls.is_empty());
}

#[test]
fn buf_write_buf() {
    const LONG: &'static [u8] = b"1 hello world, 2 hello world, 3 hello world";

    let mut writer = BufWriter::with_capacity(32, mock! {
        Ok(b"hello world"[..].into()),
        Ok(LONG.into()),
    });

    assert_eq!(Ready(11), writer.write_buf(&mut b"hello world".into_buf()).unwrap());
    assert_eq!(Ready(43), writer.write_buf(&mut LONG.into_buf()).unwrap());

    assert!(writer.get_ref().calls.is_empty());
}

#[test]
fn shutdown_flushes() {
    let mut writer = BufWriter::with_capacity(32, mock! {
        Ok(b"hello world"[..].into()),
        Ok(Flush),
    });

    assert_eq!(11, writer.write(b"hello world").unwrap());
    assert!(writer.shutdown().unwrap().is_ready());

    assert!(writer.get_ref().calls.is_empty());
}

// ===== Test utils =====

fn would_block() -> io::Error {
    io::Error::new(io::ErrorKind::WouldBlock, "would block")
}

struct Mock {
    calls: VecDeque<io::Result<Op>>,
}

enum Op {
    Data(Vec<u8>),
    Flush,
}

use self::Op::*;

impl io::Write for Mock {
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        match self.calls.pop_front() {
            Some(Ok(Op::Data(data))) => {
                let len = data.len();
                assert!(src.len() >= len, "expect={:?}; actual={:?}", data, src);
                assert_eq!(&data[..], &src[..len]);
                Ok(len)
            }
            Some(Ok(_)) => panic!(),
            Some(Err(e)) => Err(e),
            None => panic!("unexpected write"),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.calls.pop_front() {
            Some(Ok(Op::Flush)) => {
                Ok(())
            }
            Some(Ok(_)) => panic!(),
            Some(Err(e)) => Err(e),
            None => panic!("unexpected flush"),
        }
    }
}

impl AsyncWrite for Mock {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        try_nb!(self.flush());
        Ok(Ready(()))
    }

    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        let rem = buf.remaining();

        assert!(rem > 0);

        while buf.has_remaining() {
            let n = try_nb!(self.write(buf.bytes()));
            buf.advance(n);
        }

        Ok(Ready(rem - buf.remaining()))
    }
}

impl<'a> From<&'a [u8]> for Op {
    fn from(src: &'a [u8]) -> Op {
        Op::Data(src.into())
    }
}

impl From<Vec<u8>> for Op {
    fn from(src: Vec<u8>) -> Op {
        Op::Data(src)
    }
}

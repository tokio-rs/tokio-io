extern crate tokio_io;
extern crate bytes;
extern crate futures;

use tokio_io::AsyncRead;
use tokio_io::codec::{FramedRead, Decoder};

use bytes::{BytesMut, Buf, IntoBuf, BigEndian};
use futures::Stream;
use futures::Async::{Ready, NotReady};

use std::io::{self, Read};
use std::collections::VecDeque;

macro_rules! mock {
    ($($x:expr,)*) => {{
        let mut v = VecDeque::new();
        v.extend(vec![$($x),*]);
        Mock { calls: v }
    }};
}

struct U32Decoder;

impl Decoder for U32Decoder {
    type Item = u32;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<u32>> {
        if buf.len() < 4 {
            return Ok(None);
        }

        let n = buf.drain_to(4).into_buf().get_u32::<BigEndian>();
        Ok(Some(n))
    }
}

#[test]
fn read_multi_frame_in_packet() {
    let mock = mock! {
        Ok(b"\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x02".to_vec()),
    };

    let mut framed = FramedRead::new(mock, U32Decoder);
    assert_eq!(Ready(Some(0)), framed.poll().unwrap());
    assert_eq!(Ready(Some(1)), framed.poll().unwrap());
    assert_eq!(Ready(Some(2)), framed.poll().unwrap());
    assert_eq!(Ready(None), framed.poll().unwrap());
}

#[test]
fn read_multi_frame_across_packets() {
    let mock = mock! {
        Ok(b"\x00\x00\x00\x00".to_vec()),
        Ok(b"\x00\x00\x00\x01".to_vec()),
        Ok(b"\x00\x00\x00\x02".to_vec()),
    };

    let mut framed = FramedRead::new(mock, U32Decoder);
    assert_eq!(Ready(Some(0)), framed.poll().unwrap());
    assert_eq!(Ready(Some(1)), framed.poll().unwrap());
    assert_eq!(Ready(Some(2)), framed.poll().unwrap());
    assert_eq!(Ready(None), framed.poll().unwrap());
}

#[test]
fn read_not_ready() {
    let mock = mock! {
        Err(io::Error::new(io::ErrorKind::WouldBlock, "")),
        Ok(b"\x00\x00\x00\x00".to_vec()),
        Ok(b"\x00\x00\x00\x01".to_vec()),
    };

    let mut framed = FramedRead::new(mock, U32Decoder);
    assert_eq!(NotReady, framed.poll().unwrap());
    assert_eq!(Ready(Some(0)), framed.poll().unwrap());
    assert_eq!(Ready(Some(1)), framed.poll().unwrap());
    assert_eq!(Ready(None), framed.poll().unwrap());
}

#[test]
fn read_partial_then_not_ready() {
    let mock = mock! {
        Ok(b"\x00\x00".to_vec()),
        Err(io::Error::new(io::ErrorKind::WouldBlock, "")),
        Ok(b"\x00\x00\x00\x00\x00\x01\x00\x00\x00\x02".to_vec()),
    };

    let mut framed = FramedRead::new(mock, U32Decoder);
    assert_eq!(NotReady, framed.poll().unwrap());
    assert_eq!(Ready(Some(0)), framed.poll().unwrap());
    assert_eq!(Ready(Some(1)), framed.poll().unwrap());
    assert_eq!(Ready(Some(2)), framed.poll().unwrap());
    assert_eq!(Ready(None), framed.poll().unwrap());
}

#[test]
fn read_err() {
    let mock = mock! {
        Err(io::Error::new(io::ErrorKind::Other, "")),
    };

    let mut framed = FramedRead::new(mock, U32Decoder);
    assert_eq!(io::ErrorKind::Other, framed.poll().unwrap_err().kind());
}

#[test]
fn read_partial_then_err() {
    let mock = mock! {
        Ok(b"\x00\x00".to_vec()),
        Err(io::Error::new(io::ErrorKind::Other, "")),
    };

    let mut framed = FramedRead::new(mock, U32Decoder);
    assert_eq!(io::ErrorKind::Other, framed.poll().unwrap_err().kind());
}

#[test]
fn read_partial_would_block_then_err() {
    let mock = mock! {
        Ok(b"\x00\x00".to_vec()),
        Err(io::Error::new(io::ErrorKind::WouldBlock, "")),
        Err(io::Error::new(io::ErrorKind::Other, "")),
    };

    let mut framed = FramedRead::new(mock, U32Decoder);
    assert_eq!(NotReady, framed.poll().unwrap());
    assert_eq!(io::ErrorKind::Other, framed.poll().unwrap_err().kind());
}

// ===== Mock ======

struct Mock {
    calls: VecDeque<io::Result<Vec<u8>>>,
}

impl Read for Mock {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        match self.calls.pop_front() {
            Some(Ok(data)) => {
                debug_assert!(dst.len() >= data.len());
                dst[..data.len()].copy_from_slice(&data[..]);
                Ok(data.len())
            }
            Some(Err(e)) => Err(e),
            None => Ok(0),
        }
    }
}

impl AsyncRead for Mock {
}

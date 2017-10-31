extern crate tokio_io;

use tokio_io::io::BufReader;

use std::io::{self, Read, BufRead, SeekFrom, Seek};

/// A dummy reader intended at testing short-reads propagation.
pub struct ShortReader {
    lengths: Vec<usize>,
}

impl Read for ShortReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        if self.lengths.is_empty() {
            Ok(0)
        } else {
            Ok(self.lengths.remove(0))
        }
    }
}

#[test]
fn buffered_reader() {
    let inner: &[u8] = &[5, 6, 7, 0, 1, 2, 3, 4];
    let mut reader = BufReader::with_capacity(2, inner);

    let mut buf = [0, 0, 0];
    let nread = reader.read(&mut buf);
    assert_eq!(nread.unwrap(), 3);
    let b: &[_] = &[5, 6, 7];
    assert_eq!(buf, b);

    let mut buf = [0, 0];
    let nread = reader.read(&mut buf);
    assert_eq!(nread.unwrap(), 2);
    let b: &[_] = &[0, 1];
    assert_eq!(buf, b);

    let mut buf = [0];
    let nread = reader.read(&mut buf);
    assert_eq!(nread.unwrap(), 1);
    let b: &[_] = &[2];
    assert_eq!(buf, b);

    let mut buf = [0, 0, 0];
    let nread = reader.read(&mut buf);
    assert_eq!(nread.unwrap(), 1);
    let b: &[_] = &[3, 0, 0];
    assert_eq!(buf, b);

    let nread = reader.read(&mut buf);
    assert_eq!(nread.unwrap(), 1);
    let b: &[_] = &[4, 0, 0];
    assert_eq!(buf, b);

    assert_eq!(reader.read(&mut buf).unwrap(), 0);
}

#[test]
fn buffered_reader_seek() {
    let inner: &[u8] = &[5, 6, 7, 0, 1, 2, 3, 4];
    let mut reader = BufReader::with_capacity(2, io::Cursor::new(inner));

    assert_eq!(reader.seek(SeekFrom::Start(3)).ok(), Some(3));
    assert_eq!(reader.fill_buf().ok(), Some(&[0, 1][..]));
    assert_eq!(reader.seek(SeekFrom::Current(0)).ok(), Some(3));
    assert_eq!(reader.fill_buf().ok(), Some(&[0, 1][..]));
    assert_eq!(reader.seek(SeekFrom::Current(1)).ok(), Some(4));
    assert_eq!(reader.fill_buf().ok(), Some(&[1, 2][..]));
    reader.consume(1);
    assert_eq!(reader.seek(SeekFrom::Current(-2)).ok(), Some(3));
}

#[test]
fn buffered_reader_seek_underflow() {
    // gimmick reader that yields its position modulo 256 for each byte
    struct PositionReader {
        pos: u64
    }
    impl Read for PositionReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let len = buf.len();
            for x in buf {
                *x = self.pos as u8;
                self.pos = self.pos.wrapping_add(1);
            }
            Ok(len)
        }
    }
    impl Seek for PositionReader {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            match pos {
                SeekFrom::Start(n) => {
                    self.pos = n;
                }
                SeekFrom::Current(n) => {
                    self.pos = self.pos.wrapping_add(n as u64);
                }
                SeekFrom::End(n) => {
                    self.pos = u64::max_value().wrapping_add(n as u64);
                }
            }
            Ok(self.pos)
        }
    }

    let mut reader = BufReader::with_capacity(5, PositionReader { pos: 0 });
    assert_eq!(reader.fill_buf().ok(), Some(&[0, 1, 2, 3, 4][..]));
    assert_eq!(reader.seek(SeekFrom::End(-5)).ok(), Some(u64::max_value()-5));
    assert_eq!(reader.fill_buf().ok().map(|s| s.len()), Some(5));
    // the following seek will require two underlying seeks
    let expected = 9223372036854775802;
    assert_eq!(reader.seek(SeekFrom::Current(i64::min_value())).ok(), Some(expected));
    assert_eq!(reader.fill_buf().ok().map(|s| s.len()), Some(5));
    // seeking to 0 should empty the buffer.
    assert_eq!(reader.seek(SeekFrom::Current(0)).ok(), Some(expected));
    assert_eq!(reader.get_ref().pos, expected);
}

#[test]
fn read_until() {
    let inner: &[u8] = &[0, 1, 2, 1, 0];
    let mut reader = BufReader::with_capacity(2, inner);
    let mut v = Vec::new();
    reader.read_until(0, &mut v).unwrap();
    assert_eq!(v, [0]);
    v.truncate(0);
    reader.read_until(2, &mut v).unwrap();
    assert_eq!(v, [1, 2]);
    v.truncate(0);
    reader.read_until(1, &mut v).unwrap();
    assert_eq!(v, [1]);
    v.truncate(0);
    reader.read_until(8, &mut v).unwrap();
    assert_eq!(v, [0]);
    v.truncate(0);
    reader.read_until(9, &mut v).unwrap();
    assert_eq!(v, []);
}

#[test]
fn read_line() {
    let in_buf: &[u8] = b"a\nb\nc";
    let mut reader = BufReader::with_capacity(2, in_buf);
    let mut s = String::new();
    reader.read_line(&mut s).unwrap();
    assert_eq!(s, "a\n");
    s.truncate(0);
    reader.read_line(&mut s).unwrap();
    assert_eq!(s, "b\n");
    s.truncate(0);
    reader.read_line(&mut s).unwrap();
    assert_eq!(s, "c");
    s.truncate(0);
    reader.read_line(&mut s).unwrap();
    assert_eq!(s, "");
}

#[test]
fn short_reads() {
    let inner = ShortReader{lengths: vec![0, 1, 2, 0, 1, 0]};
    let mut reader = BufReader::new(inner);
    let mut buf = [0, 0];
    assert_eq!(reader.read(&mut buf).unwrap(), 0);
    assert_eq!(reader.read(&mut buf).unwrap(), 1);
    assert_eq!(reader.read(&mut buf).unwrap(), 2);
    assert_eq!(reader.read(&mut buf).unwrap(), 0);
    assert_eq!(reader.read(&mut buf).unwrap(), 1);
    assert_eq!(reader.read(&mut buf).unwrap(), 0);
    assert_eq!(reader.read(&mut buf).unwrap(), 0);
}

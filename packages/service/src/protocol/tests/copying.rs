use super::super::copy_flushed;
use std::{
    cell::RefCell,
    io::{self, Read, Write},
    rc::Rc,
};

struct Recorded(Rc<RefCell<Vec<u8>>>);
impl Write for Recorded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
// Stdout similarly hides its LineWriter from the public Write boundary.
struct Buffered(io::LineWriter<Recorded>);
impl Write for Buffered {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[test]
fn copy_flushed_exposes_binary_batches_before_the_next_read() {
    struct Batches {
        step: usize,
        recorded: Rc<RefCell<Vec<u8>>>,
    }
    impl Read for Batches {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.step += 1;
            match self.step {
                1 => Err(io::ErrorKind::Interrupted.into()),
                2 => {
                    buffer[..3].copy_from_slice(&[0, 0xff, 17]);
                    Ok(3)
                }
                3 => {
                    assert_eq!(*self.recorded.borrow(), [0, 0xff, 17]);
                    buffer[..2].copy_from_slice(&[0xfe, 0]);
                    Ok(2)
                }
                4 => {
                    assert_eq!(*self.recorded.borrow(), [0, 0xff, 17, 0xfe, 0]);
                    Ok(0)
                }
                _ => panic!("reading after EOF"),
            }
        }
    }
    let recorded = Rc::new(RefCell::new(Vec::new()));
    let reader = Batches {
        step: 0,
        recorded: recorded.clone(),
    };
    let writer = Buffered(io::LineWriter::new(Recorded(recorded.clone())));
    assert_eq!(copy_flushed(reader, writer).unwrap(), 5);
    assert_eq!(*recorded.borrow(), [0, 0xff, 17, 0xfe, 0]);
}

#[test]
fn copy_flushed_preserves_io_errors_and_eof() {
    struct Input(usize);
    impl Read for Input {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.0 += 1;
            if self.0 == 1 {
                bytes[..2].copy_from_slice(&[0, 0xff]);
                Ok(2)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "read refused",
                ))
            }
        }
    }
    struct Output {
        refuse_write: bool,
        refuse_flush: bool,
        bytes: Vec<u8>,
    }
    impl Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.refuse_write {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "write refused"));
            }
            // A short write must not discard the rest of a binary chunk.
            self.bytes.extend_from_slice(&bytes[..1]);
            Ok(1)
        }
        fn flush(&mut self) -> io::Result<()> {
            match self.refuse_flush {
                true => Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "flush refused",
                )),
                false => Ok(()),
            }
        }
    }
    for (write, flush, reads, expected, text) in [
        (true, false, 1, io::ErrorKind::BrokenPipe, "write refused"),
        (
            false,
            true,
            1,
            io::ErrorKind::PermissionDenied,
            "flush refused",
        ),
        (
            false,
            false,
            2,
            io::ErrorKind::ConnectionReset,
            "read refused",
        ),
    ] {
        let mut reader = Input(0);
        let mut writer = Output {
            refuse_write: write,
            refuse_flush: flush,
            bytes: Vec::new(),
        };
        let error = copy_flushed(&mut reader, &mut writer).unwrap_err();
        assert_eq!(error.kind(), expected);
        assert_eq!(error.to_string(), text);
        assert_eq!(reader.0, reads, "continued input after an output failure");
        assert_eq!(writer.bytes, if write { vec![] } else { vec![0, 0xff] });
    }
    let mut output = Vec::new();
    assert_eq!(copy_flushed(io::empty(), &mut output).unwrap(), 0);
    assert!(output.is_empty());
    let mut writer = Output {
        refuse_write: false,
        refuse_flush: true,
        bytes: Vec::new(),
    };
    assert_eq!(
        copy_flushed(io::empty(), &mut writer).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

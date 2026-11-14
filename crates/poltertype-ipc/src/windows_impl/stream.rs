//! `PipeStream` — one direction's `Read`/`Write` over a shared pipe.

use std::io::{Read, Write};
use std::sync::Arc;

use super::io;
use super::pipe::Pipe;

/// A reader or a writer over a shared [`Pipe`]. Both may be in use from
/// different threads at once.
pub(super) struct PipeStream(pub(super) Arc<Pipe>);

impl Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.0.is_closed() {
            return Ok(0);
        }
        io::read(self.0.handle(), buf, None)
    }
}

impl Write for PipeStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.0.is_closed() {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        }
        io::write(self.0.handle(), buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

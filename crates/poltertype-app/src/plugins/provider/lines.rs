//! One line at a time off the channel, never more than the cap.

use std::io::{BufRead, ErrorKind};

/// What reading a line found.
pub(super) enum Line {
    Text(String),
    /// Past `cap` bytes before a newline. The rest is not read.
    TooLong,
    /// Closed, or failed: either way there is nothing more to read.
    End,
}

/// Read up to and including `\n`, giving up past `cap` bytes so a
/// provider cannot make PolterType buffer without bound.
pub(super) fn read_bounded_line(reader: &mut dyn BufRead, cap: usize) -> Line {
    let mut out: Vec<u8> = Vec::new();
    loop {
        let buf = match reader.fill_buf() {
            Ok([]) => return Line::End,
            Ok(b) => b,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Line::End,
        };
        let (take, done) = match buf.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (buf.len(), false),
        };
        if out.len() + take > cap + 1 {
            return Line::TooLong;
        }
        out.extend_from_slice(&buf[..take]);
        reader.consume(take);
        if done {
            return match String::from_utf8(out) {
                Ok(s) => Line::Text(s),
                // Not text: as useless as a line that is not JSON.
                Err(_) => Line::Text(String::new()),
            };
        }
    }
}

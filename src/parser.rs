//! Incremental parser for the systemd journal export format.
//!
//! The parser keeps the unparsed tail of the stream and the fields collected
//! for the entry in progress, so parsing is linear in the stream length no
//! matter how the stream is chunked.

use memchr::memchr;
use std::fmt;

/// A parsed entry: `(wanted field index, value)` pairs in stream order.
pub type Entry = Vec<(usize, String)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Binary field data was not followed by a newline.
    MalformedBinaryEntry { field: String, size: u64, found: u8 },
    /// The stream ended inside a binary field.
    TruncatedBinaryField,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedBinaryEntry { field, size, found } => write!(
                f,
                "Binary field {field} of {size} bytes is not followed by a newline, found 0x{found:02x}"
            ),
            Self::TruncatedBinaryField => write!(f, "Stream ended inside a binary field"),
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug)]
pub struct Parser {
    wanted: Vec<Vec<u8>>,
    buf: Vec<u8>,
    /// Start of the next unparsed field.
    pos: usize,
    /// Fields collected for the entry in progress.
    entry: Entry,
    /// Buffer length a binary field needs before parsing can continue.
    need: usize,
}

enum Step {
    Field,
    EntryEnd,
    Incomplete,
}

impl Parser {
    pub fn new<S: AsRef<str>>(fields: impl IntoIterator<Item = S>) -> Self {
        Self {
            wanted: fields
                .into_iter()
                .map(|f| f.as_ref().as_bytes().to_vec())
                .collect(),
            buf: Vec::new(),
            pos: 0,
            entry: Vec::new(),
            need: 0,
        }
    }

    /// Parse `data` and return the entries it completed. Entries without any
    /// wanted field are dropped.
    pub fn feed(&mut self, data: &[u8]) -> Result<Vec<Entry>, ParseError> {
        self.buf.extend_from_slice(data);
        let mut entries = Vec::new();
        while self.buf.len() >= self.need {
            match self.step()? {
                Step::Field => {}
                Step::EntryEnd => {
                    if !self.entry.is_empty() {
                        entries.push(std::mem::take(&mut self.entry));
                    }
                }
                Step::Incomplete => break,
            }
        }
        // Drop consumed data once it outweighs the pending tail, which keeps
        // the copying amortized linear.
        if self.pos > self.buf.len() / 2 {
            self.buf.drain(..self.pos);
            self.need = self.need.saturating_sub(self.pos);
            self.pos = 0;
        }
        Ok(entries)
    }

    /// Return a trailing entry that was cut off at the end of the stream.
    pub fn close(&mut self) -> Result<Vec<Entry>, ParseError> {
        if self.buf.len() < self.need {
            return Err(ParseError::TruncatedBinaryField);
        }
        if self.pos >= self.buf.len() && self.entry.is_empty() {
            return Ok(Vec::new());
        }
        let entries = self.feed(b"\n\n")?;
        // Simple fields always end at the terminator, so leftover data is a
        // binary field header that was cut off.
        if self.pos < self.buf.len() {
            return Err(ParseError::TruncatedBinaryField);
        }
        Ok(entries)
    }

    fn step(&mut self) -> Result<Step, ParseError> {
        let buf = &self.buf[self.pos..];
        let Some(line_end) = memchr(b'\n', buf) else {
            self.need = 0;
            return Ok(Step::Incomplete);
        };
        if line_end == 0 {
            self.pos += 1;
            return Ok(Step::EntryEnd);
        }

        let line = &buf[..line_end];
        let (name, data_start, data_end) = match memchr(b'=', line) {
            Some(sep) => (&line[..sep], sep + 1, line_end),
            None => {
                // Binary field: name, newline, little endian 64-bit size, data, newline
                let data_start = line_end + 9;
                if buf.len() < data_start {
                    self.need = self.pos + data_start;
                    return Ok(Step::Incomplete);
                }
                let size = u64::from_le_bytes(buf[line_end + 1..data_start].try_into().unwrap());
                let data_end = data_start.saturating_add(size.min(usize::MAX as u64) as usize);
                if buf.len() <= data_end {
                    self.need = self.pos.saturating_add(data_end).saturating_add(1);
                    return Ok(Step::Incomplete);
                }
                if buf[data_end] != b'\n' {
                    return Err(ParseError::MalformedBinaryEntry {
                        field: String::from_utf8_lossy(line).into_owned(),
                        size,
                        found: buf[data_end],
                    });
                }
                (line, data_start, data_end)
            }
        };

        if let Some(index) = self.wanted.iter().position(|w| w == name) {
            let value = String::from_utf8_lossy(&buf[data_start..data_end]).into_owned();
            // Last occurrence of a repeated field wins
            match self.entry.iter_mut().find(|(i, _)| *i == index) {
                Some(slot) => slot.1 = value,
                None => self.entry.push((index, value)),
            }
        }
        self.pos += data_end + 1;
        self.need = 0;
        Ok(Step::Field)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(fields: &[&str], data: &[u8], chunk: usize) -> Result<Vec<Entry>, ParseError> {
        let mut parser = Parser::new(fields);
        let mut entries = Vec::new();
        for piece in data.chunks(chunk.max(1)) {
            entries.extend(parser.feed(piece)?);
        }
        entries.extend(parser.close()?);
        Ok(entries)
    }

    const EXPORT: &[u8] = b"__CURSOR=cursor1\n\
        MESSAGE\n\x0e\x00\x00\x00\x00\x00\x00\x00Hello,\nworld!\n\n\
        AFTER=after\n\n\
        __CURSOR=cursor2\n\
        MESSAGE=Hello again, world!\n\n\
        MESSAGE=No cursor\n";

    #[test]
    fn simple_entry() {
        let entries = parse_all(&["MESSAGE"], b"MESSAGE=Hello, world!\n\n", 1024).unwrap();
        assert_eq!(entries, vec![vec![(0, "Hello, world!".to_string())]]);
    }

    #[test]
    fn split_feeds_any_chunking() {
        let expected = vec![
            vec![
                (0, "cursor1".to_string()),
                (1, "Hello,\nworld!\n".to_string()),
            ],
            vec![
                (0, "cursor2".to_string()),
                (1, "Hello again, world!".to_string()),
            ],
            vec![(1, "No cursor".to_string())],
        ];
        for chunk in [1, 7, EXPORT.len()] {
            assert_eq!(
                parse_all(&["__CURSOR", "MESSAGE"], EXPORT, chunk).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn skips_unwanted_fields_including_binary_with_terminator_in_data() {
        let data = b"ID=1\nOTHER\n\x02\x00\x00\x00\x00\x00\x00\x00\n\n\nMESSAGE=Hi\n\n";
        assert_eq!(
            parse_all(&["MESSAGE"], data, 1024).unwrap(),
            vec![vec![(0, "Hi".to_string())]]
        );
    }

    #[test]
    fn entries_without_wanted_fields_are_dropped() {
        assert!(
            parse_all(&["MESSAGE"], b"ID=1\n\nID=2\n\n", 1024)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn repeated_field_last_wins() {
        for data in [
            &b"MESSAGE=first\nMESSAGE=last\n\n"[..],
            &b"MESSAGE=first\nBIN\n\x01\x00\x00\x00\x00\x00\x00\x00\n\nMESSAGE=last\n\n"[..],
        ] {
            assert_eq!(
                parse_all(&["MESSAGE"], data, 1024).unwrap(),
                vec![vec![(0, "last".to_string())]]
            );
        }
    }

    #[test]
    fn invalid_utf8_is_replaced() {
        let entries = parse_all(&["MESSAGE"], b"MESSAGE=Hello, \xff world!\n\n", 1024).unwrap();
        assert_eq!(entries[0][0].1, "Hello, \u{fffd} world!");
    }

    #[test]
    fn malformed_binary_separator() {
        let data = b"ID=1\nMESSAGE\n\x0d\x00\x00\x00\x00\x00\x00\x00Hello, world!AFTER=after\n\n";
        assert!(matches!(
            parse_all(&["MESSAGE"], data, 1024),
            Err(ParseError::MalformedBinaryEntry {
                field,
                size: 13,
                found: b'A'
            }) if field == "MESSAGE"
        ));
    }

    #[test]
    fn truncated_binary_field() {
        for data in [
            &b"MESSAGE"[..],
            &b"MESSAGE\n\x0d\x00\x00\x00"[..],
            &b"MESSAGE\n\x0d\x00\x00\x00\x00\x00\x00\x00Hello"[..],
            &b"MESSAGE\n\x05\x00\x00\x00\x00\x00\x00\x00Hello"[..],
        ] {
            let mut parser = Parser::new(["MESSAGE"]);
            assert!(parser.feed(data).unwrap().is_empty());
            assert_eq!(parser.close(), Err(ParseError::TruncatedBinaryField));
        }
    }

    #[test]
    fn binary_field_split_after_completed_entry() {
        let payload = b"a\n\nbcdefghijk";
        let mut first = b"MESSAGE=".to_vec();
        first.extend_from_slice(&[b'x'; 40]);
        first.extend_from_slice(b"\n\nMESSAGE\n");
        first.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        first.extend_from_slice(&payload[..6]);
        let mut second = payload[6..].to_vec();
        second.extend_from_slice(b"\n\n");

        let mut parser = Parser::new(["MESSAGE"]);
        assert_eq!(
            parser.feed(&first).unwrap(),
            vec![vec![(0, "x".repeat(40))]]
        );
        assert_eq!(
            parser.feed(&second).unwrap(),
            vec![vec![(0, String::from_utf8_lossy(payload).into_owned())]]
        );
        assert!(parser.close().unwrap().is_empty());
    }

    #[test]
    fn large_binary_field_in_small_feeds_stays_small() {
        let payload = vec![b'x'; 8 << 20];
        let mut data = b"MESSAGE\n".to_vec();
        data.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        data.extend_from_slice(&payload);
        data.extend_from_slice(b"\n\nAFTER=1\n\n");
        let mut parser = Parser::new(["MESSAGE"]);
        let mut entries = Vec::new();
        for piece in data.chunks(4096) {
            entries.extend(parser.feed(piece).unwrap());
        }
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0][0].1.len(), payload.len());
        assert_eq!(
            parser.feed(b"MESSAGE=next\n\n").unwrap(),
            vec![vec![(0, "next".to_string())]]
        );
        assert!(parser.buf.len() < 8192);
    }
}

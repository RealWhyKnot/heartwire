use crate::heart_rate::MAX_BPM;

pub const MAX_LINE: usize = 256;

#[derive(Debug, PartialEq, Eq)]
pub enum PicoLine<'a> {
    Reading(u16),
    Note(&'a str),
    Repl,
    Other,
}

#[derive(Default)]
pub struct LineBuffer {
    line: Vec<u8>,
    synced: bool,
}

impl LineBuffer {
    pub fn feed(&mut self, bytes: &[u8], mut each: impl FnMut(&[u8])) {
        for &byte in bytes {
            if byte == b'\n' {
                if self.synced {
                    each(&self.line);
                }
                self.synced = true;
                self.line.clear();
            } else if self.synced && self.line.len() < MAX_LINE {
                self.line.push(byte);
            }
        }
    }
}

pub fn parse_pico_line(raw: &[u8]) -> PicoLine<'_> {
    let Ok(text) = std::str::from_utf8(raw) else {
        return PicoLine::Other;
    };
    let text = text.trim();
    if let Some(note) = text.strip_prefix('#') {
        return PicoLine::Note(note.trim());
    }
    if text.starts_with("Type \"help()\"") {
        return PicoLine::Repl;
    }
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return PicoLine::Other;
    }
    match text.parse::<u16>() {
        Ok(bpm) if (1..=MAX_BPM).contains(&bpm) => PicoLine::Reading(bpm),
        _ => PicoLine::Other,
    }
}

pub fn note_text(note: &str) -> String {
    if let Some(name) = note.strip_prefix("found ") {
        return format!("Pico found {name}");
    }
    match note {
        "subscribed" => "Pico connected to the strap".into(),
        "no strap found, rescanning" => "Pico is searching for a strap".into(),
        "no skin contact" => "Pico: no skin contact".into(),
        "strap disconnected" => "Pico lost the strap".into(),
        other => format!("Pico: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pico_lines() {
        assert_eq!(parse_pico_line(b"72\r\n"), PicoLine::Reading(72));
        assert_eq!(
            parse_pico_line(b"# found COOSPO\r\n"),
            PicoLine::Note("found COOSPO")
        );
        assert_eq!(parse_pico_line(b"0\n"), PicoLine::Other);
        assert_eq!(parse_pico_line(b"301\n"), PicoLine::Other);
        assert_eq!(parse_pico_line(b"-5\n"), PicoLine::Other);
        assert_eq!(parse_pico_line(b">>> \n"), PicoLine::Other);
        assert_eq!(parse_pico_line(b"\xff\xfe\n"), PicoLine::Other);
        assert_eq!(parse_pico_line(b"\n"), PicoLine::Other);
        assert_eq!(
            parse_pico_line(b"Type \"help()\" for more information.\r"),
            PicoLine::Repl
        );
    }

    fn lines(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut buffer = LineBuffer::default();
        let mut out = Vec::new();
        for chunk in chunks {
            buffer.feed(chunk, |line| out.push(line.to_vec()));
        }
        out
    }

    #[test]
    fn the_partial_line_before_the_first_newline_is_dropped() {
        assert_eq!(
            lines(&[b"nd, rescanning\r\n# no strap found\r\n72\r\n"]),
            [b"# no strap found\r".to_vec(), b"72\r".to_vec()]
        );
    }

    #[test]
    fn lines_join_across_reads() {
        assert_eq!(
            lines(&[b"\n7", b"2\r", b"\n# subs", b"cribed\n"]),
            [b"72\r".to_vec(), b"# subscribed".to_vec()]
        );
    }

    #[test]
    fn overlong_lines_are_cut_at_the_limit() {
        let mut long = b"\n".to_vec();
        long.extend(std::iter::repeat_n(b'9', MAX_LINE * 3));
        long.extend_from_slice(b"\n60\n");
        let out = lines(&[&long]);
        assert_eq!(out[0].len(), MAX_LINE);
        assert_eq!(parse_pico_line(&out[0]), PicoLine::Other);
        assert_eq!(parse_pico_line(&out[1]), PicoLine::Reading(60));
    }

    #[test]
    fn notes_read_as_status_lines() {
        assert_eq!(note_text("found COOSPO HW807"), "Pico found COOSPO HW807");
        assert_eq!(note_text("subscribed"), "Pico connected to the strap");
        assert_eq!(note_text("error: OSError(5)"), "Pico: error: OSError(5)");
    }
}

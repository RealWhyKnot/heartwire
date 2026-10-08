use crate::heart_rate::MAX_BPM;

#[derive(Debug, PartialEq, Eq)]
pub enum PicoLine<'a> {
    Reading(u16),
    Note(&'a str),
    Other,
}

pub fn parse_pico_line(raw: &[u8]) -> PicoLine<'_> {
    let Ok(text) = std::str::from_utf8(raw) else {
        return PicoLine::Other;
    };
    let text = text.trim();
    if let Some(note) = text.strip_prefix('#') {
        return PicoLine::Note(note.trim());
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
    }

    #[test]
    fn notes_read_as_status_lines() {
        assert_eq!(note_text("found COOSPO HW807"), "Pico found COOSPO HW807");
        assert_eq!(note_text("subscribed"), "Pico connected to the strap");
        assert_eq!(note_text("error: OSError(5)"), "Pico: error: OSError(5)");
    }
}

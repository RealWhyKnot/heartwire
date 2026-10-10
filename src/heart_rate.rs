pub const MAX_BPM: u16 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measurement {
    pub bpm: u16,
    pub contact: Option<bool>,
}

pub fn parse_measurement(data: &[u8]) -> Option<Measurement> {
    let flags = *data.first()?;
    let bpm = if flags & 0x01 != 0 {
        u16::from_le_bytes([*data.get(1)?, *data.get(2)?])
    } else {
        u16::from(*data.get(1)?)
    };
    let contact = (flags & 0x04 != 0).then_some(flags & 0x02 != 0);
    Some(Measurement { bpm, contact })
}

pub fn usable(m: Measurement) -> Option<u16> {
    (m.contact != Some(false) && (1..=MAX_BPM).contains(&m.bpm)).then_some(m.bpm)
}

pub fn bpm_from_f64(value: f64) -> Option<u16> {
    if !value.is_finite() {
        return None;
    }
    let bpm = value.round();
    (1.0..=f64::from(MAX_BPM))
        .contains(&bpm)
        .then_some(bpm as u16)
}

pub fn parse_bpm_text(text: &str) -> Option<u16> {
    bpm_from_f64(text.trim().parse().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_bit_rate() {
        assert_eq!(
            parse_measurement(&[0x00, 72]),
            Some(Measurement {
                bpm: 72,
                contact: None
            })
        );
    }

    #[test]
    fn sixteen_bit_rate_is_little_endian() {
        assert_eq!(parse_measurement(&[0x01, 0x2C, 0x01]).unwrap().bpm, 300);
    }

    #[test]
    fn short_packets_are_rejected() {
        assert_eq!(parse_measurement(&[]), None);
        assert_eq!(parse_measurement(&[0x00]), None);
        assert_eq!(parse_measurement(&[0x01, 0x48]), None);
    }

    #[test]
    fn contact_bits() {
        let off = parse_measurement(&[0x04, 70]).unwrap();
        assert_eq!(off.contact, Some(false));
        assert_eq!(usable(off), None);
        let on = parse_measurement(&[0x06, 70]).unwrap();
        assert_eq!(on.contact, Some(true));
        assert_eq!(usable(on), Some(70));
    }

    #[test]
    fn rr_and_energy_fields_do_not_disturb_the_rate() {
        let packet = [0x18, 64, 0x10, 0x00, 0x00, 0x04];
        assert_eq!(usable(parse_measurement(&packet).unwrap()), Some(64));
    }

    #[test]
    fn zero_is_not_a_reading() {
        assert_eq!(usable(parse_measurement(&[0x00, 0]).unwrap()), None);
    }

    #[test]
    fn bpm_text() {
        assert_eq!(parse_bpm_text("60"), Some(60));
        assert_eq!(parse_bpm_text(" 72\n"), Some(72));
        assert_eq!(parse_bpm_text("71.6"), Some(72));
        assert_eq!(parse_bpm_text("0"), None);
        assert_eq!(parse_bpm_text("400"), None);
        assert_eq!(parse_bpm_text("abc"), None);
        assert_eq!(parse_bpm_text("NaN"), None);
        assert_eq!(parse_bpm_text(""), None);
    }

    #[test]
    fn bpm_numbers() {
        assert_eq!(bpm_from_f64(71.5), Some(72));
        assert_eq!(bpm_from_f64(0.4), None);
        assert_eq!(bpm_from_f64(300.4), Some(300));
        assert_eq!(bpm_from_f64(300.5), None);
        assert_eq!(bpm_from_f64(f64::INFINITY), None);
        assert_eq!(bpm_from_f64(-72.0), None);
    }
}

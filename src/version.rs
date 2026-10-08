pub const VERSION: &str = match option_env!("HEARTWIRE_VERSION") {
    Some(v) => v,
    None => "dev",
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Dev,
    Beta,
    Release,
}

impl Channel {
    pub fn parse(text: Option<&str>) -> Channel {
        match text.map(str::trim) {
            Some(t) if t.eq_ignore_ascii_case("release") => Channel::Release,
            Some(t) if t.eq_ignore_ascii_case("beta") => Channel::Beta,
            _ => Channel::Dev,
        }
    }

    pub fn current() -> Channel {
        Channel::parse(option_env!("HEARTWIRE_CHANNEL"))
    }

    pub fn name(self) -> &'static str {
        match self {
            Channel::Dev => "dev",
            Channel::Beta => "beta",
            Channel::Release => "release",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Channel;

    #[test]
    fn channels() {
        assert_eq!(Channel::parse(Some("release")), Channel::Release);
        assert_eq!(Channel::parse(Some("Beta")), Channel::Beta);
        assert_eq!(Channel::parse(Some("")), Channel::Dev);
        assert_eq!(Channel::parse(None), Channel::Dev);
    }
}

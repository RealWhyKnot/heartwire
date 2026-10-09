use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arg {
    Float(f32),
    Bool(bool),
}

pub fn encode(address: &str, arg: Arg, out: &mut Vec<u8>) {
    out.clear();
    push_padded(out, address.as_bytes());
    match arg {
        Arg::Float(value) => {
            push_padded(out, b",f");
            out.extend_from_slice(&value.to_be_bytes());
        }
        Arg::Bool(true) => push_padded(out, b",T"),
        Arg::Bool(false) => push_padded(out, b",F"),
    }
}

fn push_padded(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
    out.resize(out.len() + 4 - bytes.len() % 4, 0);
}

pub fn resolve(host: &str, port: u16) -> SocketAddr {
    let host = host.trim();
    if let Ok(ip) = host.parse::<IpAddr>() {
        return SocketAddr::new(ip, port);
    }
    if !host.is_empty()
        && let Ok(mut found) = (host, port).to_socket_addrs()
        && let Some(addr) = found.next()
    {
        return addr;
    }
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

pub struct Sender {
    socket: Option<UdpSocket>,
    target: SocketAddr,
    buf: Vec<u8>,
    failures: Failures,
}

impl Sender {
    pub fn new(target: SocketAddr) -> Self {
        let mut sender = Sender {
            socket: None,
            target,
            buf: Vec::with_capacity(64),
            failures: Failures::default(),
        };
        sender.retarget(target);
        sender
    }

    pub fn retarget(&mut self, target: SocketAddr) {
        let same_family = self
            .socket
            .as_ref()
            .and_then(|s| s.local_addr().ok())
            .is_some_and(|local| local.is_ipv4() == target.is_ipv4());
        if !same_family {
            let bind: SocketAddr = if target.is_ipv4() {
                (Ipv4Addr::UNSPECIFIED, 0).into()
            } else {
                (std::net::Ipv6Addr::UNSPECIFIED, 0).into()
            };
            self.socket = match UdpSocket::bind(bind) {
                Ok(socket) => Some(socket),
                Err(error) => {
                    crate::log::write(&format!("osc socket: {error}"));
                    None
                }
            };
        }
        self.target = target;
    }

    pub fn send(&mut self, address: &str, arg: Arg) {
        if address.is_empty() {
            return;
        }
        let Some(socket) = &self.socket else { return };
        encode(address, arg, &mut self.buf);
        let result = socket.send_to(&self.buf, self.target);
        if let Some(line) = self.failures.note(result.err(), self.target) {
            crate::log::write(&line);
        }
    }
}

#[derive(Default)]
struct Failures {
    failing: bool,
}

impl Failures {
    fn note(&mut self, error: Option<std::io::Error>, target: SocketAddr) -> Option<String> {
        match (error, self.failing) {
            (Some(error), false) => {
                self.failing = true;
                Some(format!("osc send to {target}: {error}"))
            }
            (None, true) => {
                self.failing = false;
                Some(format!("osc send to {target} works again"))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_message_matches_the_spec_layout() {
        let mut out = Vec::new();
        encode("/avatar/parameters/hr_percent", Arg::Float(0.5), &mut out);
        let mut expected = b"/avatar/parameters/hr_percent\0\0\0".to_vec();
        expected.extend_from_slice(b",f\0\0");
        expected.extend_from_slice(&0.5f32.to_be_bytes());
        assert_eq!(out, expected);
        assert_eq!(out.len() % 4, 0);
    }

    #[test]
    fn address_on_a_four_byte_boundary_gets_a_full_pad() {
        let mut out = Vec::new();
        encode("/abc", Arg::Bool(true), &mut out);
        assert_eq!(out, b"/abc\0\0\0\0,T\0\0");
    }

    #[test]
    fn false_bool_has_no_payload() {
        let mut out = Vec::new();
        encode("/a", Arg::Bool(false), &mut out);
        assert_eq!(out, b"/a\0\0,F\0\0");
    }

    #[test]
    fn resolve_falls_back_to_localhost() {
        assert_eq!(resolve("", 9000), "127.0.0.1:9000".parse().unwrap());
        assert_eq!(
            resolve("192.168.1.20", 9001),
            "192.168.1.20:9001".parse().unwrap()
        );
        assert_eq!(
            resolve("not a host name!", 9000),
            "127.0.0.1:9000".parse().unwrap()
        );
    }

    #[test]
    fn a_failure_streak_is_logged_once_and_so_is_the_recovery() {
        let target: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let fail = || Some(std::io::Error::from(std::io::ErrorKind::ConnectionReset));
        let mut failures = Failures::default();
        let lines: Vec<String> = [fail(), fail(), fail(), None, None, fail()]
            .into_iter()
            .filter_map(|e| failures.note(e, target))
            .collect();
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].starts_with("osc send to 127.0.0.1:9000:"));
        assert_eq!(lines[1], "osc send to 127.0.0.1:9000 works again");
    }

    #[test]
    fn sender_delivers_to_a_local_socket() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut sender = Sender::new(receiver.local_addr().unwrap());
        sender.send("/x", Arg::Float(1.0));
        let mut buf = [0u8; 64];
        let n = receiver.recv(&mut buf).unwrap();
        let mut expected = Vec::new();
        encode("/x", Arg::Float(1.0), &mut expected);
        assert_eq!(&buf[..n], expected.as_slice());
    }
}

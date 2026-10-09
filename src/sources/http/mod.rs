mod request;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use super::{Context, Wake};
use crate::heart_rate;
use request::{MAX_TOTAL, Request, parse_request};

const REQUEST_TIME: Duration = Duration::from_secs(5);

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn read_request(stream: &mut TcpStream, limit: Duration) -> Request {
    let deadline = Instant::now() + limit;
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 1024];
    loop {
        match parse_request(&buf) {
            Request::Incomplete => {}
            done => return done,
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || buf.len() >= MAX_TOTAL {
            return Request::Bad;
        }
        let _ = stream.set_read_timeout(Some(left));
        let room = chunk.len().min(MAX_TOTAL - buf.len());
        match stream.read(&mut chunk[..room]) {
            Ok(0) | Err(_) => return Request::Bad,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

fn handle(mut stream: TcpStream, ctx: &Context, limit: Duration) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let request = read_request(&mut stream, limit);
    match request {
        Request::Get => respond(&mut stream, "200 OK", "hr-osc http server"),
        Request::Post(body) => match std::str::from_utf8(&body)
            .ok()
            .and_then(heart_rate::parse_bpm_text)
        {
            Some(bpm) => {
                ctx.reading(bpm);
                respond(&mut stream, "200 OK", "ok");
            }
            None => respond(&mut stream, "400 Bad Request", "error parsing heartrate"),
        },
        Request::Other => respond(&mut stream, "405 Method Not Allowed", ""),
        Request::Bad | Request::Incomplete => respond(&mut stream, "400 Bad Request", ""),
    }
}

pub fn run(ctx: Context, port: u16) {
    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    let listener = loop {
        match TcpListener::bind(addr) {
            Ok(listener) => break listener,
            Err(error) => {
                ctx.status(format!("Port {port} is busy, retrying"));
                let _ = crate::log::write_changed(&format!("http bind {addr}: {error}"));
                if ctx.sleep(Duration::from_secs(2)) {
                    return;
                }
            }
        }
    };
    let bound = listener.local_addr().map_or(port, |a| a.port());
    ctx.set_wake(Wake::Connect(SocketAddr::from((
        Ipv4Addr::LOCALHOST,
        bound,
    ))));
    ctx.status(format!("Listening on port {bound}"));
    crate::log::write(&format!("http listening on port {bound}"));
    for stream in listener.incoming() {
        if ctx.stopped() {
            return;
        }
        if let Ok(stream) = stream {
            handle(stream, &ctx, REQUEST_TIME);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use request::Request;

    #[test]
    fn serves_readings_over_a_real_socket() {
        let (ctx, rx, stop) = Context::test();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let worker = std::thread::spawn(move || run(ctx, port));
        let mut reply = String::new();
        for _ in 0..50 {
            if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
                s.write_all(b"POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\n88")
                    .unwrap();
                s.read_to_string(&mut reply).unwrap();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(reply.ends_with("ok"));
        assert!(rx.iter().any(|e| e.reading() == Some(88)));
        stop.stop();
        worker.join().unwrap();
    }

    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    #[test]
    fn an_endless_body_is_cut_off_at_the_size_cap() {
        let (mut client, mut server) = pair();
        let sender = std::thread::spawn(move || {
            let _ = client.write_all(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n");
            let junk = vec![b'f'; 4096];
            for _ in 0..64 {
                if client.write_all(&junk).is_err() {
                    break;
                }
            }
        });
        let start = Instant::now();
        assert_eq!(
            read_request(&mut server, Duration::from_secs(10)),
            Request::Bad
        );
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "cut by size, not by time"
        );
        drop(server);
        sender.join().unwrap();
    }

    #[test]
    fn a_trickling_client_is_cut_off_at_the_deadline() {
        let (mut client, mut server) = pair();
        let sender = std::thread::spawn(move || {
            for byte in b"POST / HTTP/1.1\r\nContent-Length: 2\r\n" {
                if client.write_all(&[*byte]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        });
        let start = Instant::now();
        assert_eq!(
            read_request(&mut server, Duration::from_millis(300)),
            Request::Bad
        );
        let took = start.elapsed();
        assert!(
            took >= Duration::from_millis(300) && took < Duration::from_secs(2),
            "{took:?}"
        );
        drop(server);
        sender.join().unwrap();
    }
}

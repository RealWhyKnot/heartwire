mod request;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::{Context, Wake};
use crate::heart_rate;
use request::MAX_TOTAL;
pub use request::{Request, parse_request};

const REQUEST_TIME: Duration = Duration::from_secs(5);
const MAX_CLIENTS: usize = 8;

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
    serve(&listener, &ctx, REQUEST_TIME);
}

fn serve(listener: &TcpListener, ctx: &Context, limit: Duration) {
    let busy = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        if ctx.stopped() {
            return;
        }
        let Ok(mut stream) = stream else {
            continue;
        };
        if busy.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
            busy.fetch_sub(1, Ordering::AcqRel);
            let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
            respond(&mut stream, "503 Service Unavailable", "");
            continue;
        }
        let ctx = ctx.clone();
        let count = busy.clone();
        let spawned = std::thread::Builder::new()
            .name("http-client".into())
            .stack_size(64 * 1024)
            .spawn(move || {
                handle(stream, &ctx, limit);
                count.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            busy.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Event;

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

    type Server = (
        u16,
        std::sync::mpsc::Receiver<Event>,
        crate::sources::Stopper,
        std::thread::JoinHandle<()>,
    );

    fn started() -> Server {
        let (ctx, rx, stop) = Context::test();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let worker = std::thread::spawn(move || run(ctx, port));
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Event::Status { text, .. }) = rx.recv_timeout(Duration::from_millis(100))
                && text.starts_with("Listening")
            {
                return (port, rx, stop, worker);
            }
        }
        panic!("the server never started");
    }

    fn post(port: u16, bpm: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let request = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{bpm}",
            bpm.len()
        );
        s.write_all(request.as_bytes()).unwrap();
        let mut reply = String::new();
        let _ = s.read_to_string(&mut reply);
        reply
    }

    #[test]
    fn a_silent_client_does_not_hold_up_a_reading() {
        let (port, rx, stop, worker) = started();
        let idle = TcpStream::connect(("127.0.0.1", port)).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let start = Instant::now();
        let reply = post(port, "91");
        let took = start.elapsed();
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(took < Duration::from_secs(1), "{took:?}");
        assert!(rx.iter().any(|e| e.reading() == Some(91)));
        drop(idle);
        stop.stop();
        worker.join().unwrap();
    }

    #[test]
    fn a_silent_client_does_not_hold_up_a_stop() {
        let (port, _rx, stop, worker) = started();
        let idle = TcpStream::connect(("127.0.0.1", port)).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let start = Instant::now();
        stop.stop();
        worker.join().unwrap();
        let took = start.elapsed();
        assert!(took < Duration::from_secs(1), "{took:?}");
        drop(idle);
    }

    #[test]
    fn clients_past_the_cap_are_turned_away() {
        let (port, _rx, stop, worker) = started();
        let idle: Vec<TcpStream> = (0..MAX_CLIENTS)
            .map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap())
            .collect();
        std::thread::sleep(Duration::from_millis(200));
        let reply = post(port, "92");
        assert!(
            reply.starts_with("HTTP/1.1 503 Service Unavailable"),
            "{reply}"
        );
        drop(idle);
        std::thread::sleep(Duration::from_millis(200));
        assert!(post(port, "93").starts_with("HTTP/1.1 200 OK"));
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

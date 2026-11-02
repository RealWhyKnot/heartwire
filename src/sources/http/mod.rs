mod request;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::{Context, Wake};
use crate::heart_rate;
use request::MAX_TOTAL;
pub(crate) use request::{Request, parse_request};

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
    read_request_with(limit, Instant::now, |left, chunk| {
        let _ = stream.set_read_timeout(Some(left));
        stream.read(chunk)
    })
}

fn read_request_with(
    limit: Duration,
    now: impl Fn() -> Instant,
    mut read: impl FnMut(Duration, &mut [u8]) -> std::io::Result<usize>,
) -> Request {
    let deadline = now() + limit;
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 1024];
    loop {
        match parse_request(&buf) {
            Request::Incomplete => {}
            done => return done,
        }
        let left = deadline.saturating_duration_since(now());
        if left.is_zero() || buf.len() >= MAX_TOTAL {
            return Request::Bad;
        }
        let room = chunk.len().min(MAX_TOTAL - buf.len());
        match read(left, &mut chunk[..room]) {
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

pub(crate) fn run(ctx: &Context, port: u16) {
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
    tracing::info!(bound = %bound, "http listening on port {bound}");
    serve(&listener, ctx, REQUEST_TIME);
}

fn serve(listener: &TcpListener, ctx: &Context, limit: Duration) {
    serve_with(listener, ctx, limit, |_| {});
}

fn serve_with(
    listener: &TcpListener,
    ctx: &Context,
    limit: Duration,
    on_client: impl Fn(bool) + Send + Sync + 'static,
) {
    let busy = Arc::new(AtomicUsize::new(0));
    let on_client = Arc::new(on_client);
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
        let count = std::sync::Arc::clone(&busy);
        let on_client = Arc::clone(&on_client);
        let spawned = std::thread::Builder::new()
            .name("http-client".into())
            .stack_size(64 * 1024)
            .spawn(move || {
                on_client(true);
                handle(stream, &ctx, limit);
                count.fetch_sub(1, Ordering::AcqRel);
                on_client(false);
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
    use std::cell::Cell;
    use std::sync::mpsc::{Receiver, channel};

    struct Server {
        port: u16,
        events: Receiver<Event>,
        clients: Receiver<bool>,
        done: Receiver<()>,
        stop: crate::sources::Stopper,
        worker: std::thread::JoinHandle<()>,
    }

    impl Server {
        fn start() -> Self {
            let (ctx, events, stop) = Context::test();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            ctx.set_wake(Wake::Connect(addr));
            let (tx, clients) = channel();
            let (done_tx, done) = channel();
            let worker = std::thread::spawn(move || {
                serve_with(&listener, &ctx, Duration::from_secs(60), move |active| {
                    let _ = tx.send(active);
                });
                done_tx.send(()).unwrap();
            });
            Self {
                port: addr.port(),
                events,
                clients,
                done,
                stop,
                worker,
            }
        }

        fn idle(&self) -> TcpStream {
            let stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
            assert!(self.clients.recv_timeout(Duration::from_secs(5)).unwrap());
            stream
        }

        fn finish(self) {
            self.stop.stop();
            self.done.recv_timeout(Duration::from_secs(5)).unwrap();
            self.worker.join().unwrap();
        }
    }

    fn post(port: u16, bpm: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let request = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{bpm}",
            bpm.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut reply = String::new();
        let _ = stream.read_to_string(&mut reply);
        reply
    }

    #[test]
    fn serves_readings_over_a_real_socket() {
        let (ctx, rx, stop) = Context::test();
        let worker = std::thread::spawn(move || run(&ctx, 0));
        let Event::Status { text, .. } = rx.recv_timeout(Duration::from_secs(5)).unwrap() else {
            panic!("the server did not report its listening port");
        };
        let port = text
            .strip_prefix("Listening on port ")
            .unwrap()
            .parse()
            .unwrap();
        let reply = post(port, "88");
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(reply.ends_with("ok"));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().reading(),
            Some(88)
        );
        stop.stop();
        worker.join().unwrap();
    }

    #[test]
    fn a_silent_client_does_not_hold_up_a_reading() {
        let server = Server::start();
        let idle = server.idle();
        let reply = post(server.port, "91");
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert_eq!(
            server
                .events
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .reading(),
            Some(91)
        );
        drop(idle);
        server.finish();
    }

    #[test]
    fn a_silent_client_does_not_hold_up_a_stop() {
        let server = Server::start();
        let idle = server.idle();
        server.finish();
        drop(idle);
    }

    #[test]
    fn clients_past_the_cap_are_turned_away() {
        let server = Server::start();
        let idle: Vec<_> = (0..MAX_CLIENTS).map(|_| server.idle()).collect();
        let mut refused = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
        refused
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reply = String::new();
        refused.read_to_string(&mut reply).unwrap();
        assert!(
            reply.starts_with("HTTP/1.1 503 Service Unavailable"),
            "{reply}"
        );
        drop(idle);
        for _ in 0..MAX_CLIENTS {
            assert!(!server.clients.recv_timeout(Duration::from_secs(5)).unwrap());
        }
        assert!(post(server.port, "93").starts_with("HTTP/1.1 200 OK"));
        server.finish();
    }

    #[test]
    fn an_endless_body_is_cut_off_at_the_size_cap() {
        let now = Instant::now();
        let mut received = 0;
        let result = read_request_with(
            Duration::from_secs(10),
            || now,
            |_, chunk| {
                chunk.fill(b'f');
                if received == 0 {
                    let header = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
                    chunk[..header.len()].copy_from_slice(header);
                }
                received += chunk.len();
                Ok(chunk.len())
            },
        );
        assert_eq!(result, Request::Bad);
        assert_eq!(received, MAX_TOTAL);
    }

    #[test]
    fn a_trickling_client_is_cut_off_at_the_deadline() {
        let start = Instant::now();
        let now = Cell::new(start);
        let mut left = Vec::new();
        let result = read_request_with(
            Duration::from_millis(300),
            || now.get(),
            |remaining, chunk| {
                left.push(remaining);
                now.set(now.get() + Duration::from_millis(40));
                chunk[0] = b'P';
                Ok(1)
            },
        );
        assert_eq!(result, Request::Bad);
        assert_eq!(
            left,
            (0..8)
                .map(|i| Duration::from_millis(300 - i * 40))
                .collect::<Vec<_>>()
        );
        assert_eq!(now.get() - start, Duration::from_millis(320));
    }
}

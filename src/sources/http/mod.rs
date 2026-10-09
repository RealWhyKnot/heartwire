mod request;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use super::{Context, Wake};
use crate::heart_rate;
use request::{Request, parse_request};

fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn handle(mut stream: TcpStream, ctx: &Context) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 1024];
    let request = loop {
        match parse_request(&buf) {
            Request::Incomplete => {}
            done => break done,
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break Request::Bad,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
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
            handle(stream, &ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

const MAX_REQUEST: usize = 16 * 1024;
pub const MAX_TOTAL: usize = 3 * MAX_REQUEST;

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Incomplete,
    Bad,
    Get,
    Post(Vec<u8>),
    Other,
}

pub fn parse_request(buf: &[u8]) -> Request {
    let Some(head_end) = find(buf, b"\r\n\r\n") else {
        return if buf.len() >= MAX_REQUEST {
            Request::Bad
        } else {
            Request::Incomplete
        };
    };
    let Ok(head) = std::str::from_utf8(&buf[..head_end]) else {
        return Request::Bad;
    };
    let body = &buf[head_end + 4..];
    let mut lines = head.split("\r\n");
    let method = lines.next().and_then(|l| l.split(' ').next()).unwrap_or("");
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            match value.parse::<usize>() {
                Ok(n) if n <= MAX_REQUEST => length = Some(n),
                _ => return Request::Bad,
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.to_ascii_lowercase().contains("chunked");
        }
    }
    match method {
        "GET" | "HEAD" => Request::Get,
        "POST" | "PUT" if chunked => match dechunk(body) {
            Some(Some(data)) => Request::Post(data),
            Some(None) => Request::Incomplete,
            None => Request::Bad,
        },
        "POST" | "PUT" => {
            let n = length.unwrap_or(0);
            if body.len() < n {
                Request::Incomplete
            } else {
                Request::Post(body[..n].to_vec())
            }
        }
        "" => Request::Bad,
        _ => Request::Other,
    }
}

fn dechunk(mut body: &[u8]) -> Option<Option<Vec<u8>>> {
    let mut out = Vec::new();
    loop {
        let Some(line_end) = find(body, b"\r\n") else {
            return Some(None);
        };
        let size_text = std::str::from_utf8(&body[..line_end]).ok()?;
        let size_text = size_text.split(';').next()?.trim();
        let size = usize::from_str_radix(size_text, 16).ok()?;
        if size > MAX_REQUEST || out.len() + size > MAX_REQUEST {
            return None;
        }
        body = &body[line_end + 2..];
        if size == 0 {
            return Some(Some(out));
        }
        if body.len() < size + 2 {
            return Some(None);
        }
        out.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_style_post() {
        let req = b"POST / HTTP/1.1\r\nHost: localhost:8080\r\nContent-Length: 2\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\n60";
        assert_eq!(parse_request(req), Request::Post(b"60".to_vec()));
    }

    #[test]
    fn body_split_across_reads() {
        let req = b"POST / HTTP/1.1\r\nContent-Length: 3\r\n\r\n12";
        assert_eq!(parse_request(req), Request::Incomplete);
        assert_eq!(
            parse_request(b"POST / HTTP/1.1\r\nConte"),
            Request::Incomplete
        );
    }

    #[test]
    fn chunked_post() {
        let req =
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n7\r\n1\r\n2\r\n0\r\n\r\n";
        assert_eq!(parse_request(req), Request::Post(b"72".to_vec()));
        let partial = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n7";
        assert_eq!(parse_request(partial), Request::Incomplete);
    }

    #[test]
    fn get_and_garbage() {
        assert_eq!(parse_request(b"GET / HTTP/1.1\r\n\r\n"), Request::Get);
        assert_eq!(parse_request(b"DELETE / HTTP/1.1\r\n\r\n"), Request::Other);
        assert_eq!(
            parse_request(b"POST / HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n"),
            Request::Bad
        );
        assert_eq!(parse_request(&[b'a'; MAX_REQUEST]), Request::Bad);
    }
}

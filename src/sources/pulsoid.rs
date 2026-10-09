use std::net::TcpStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tungstenite::Message;

use super::{Context, Wake};
use crate::heart_rate;

const RPC_URL: &str = "https://api.stromno.com/v1/api/public/rpc";
const RETRY: Duration = Duration::from_secs(5);
const SILENCE: Duration = Duration::from_secs(90);

pub fn run(ctx: &Context, widget: &str) {
    if widget.is_empty() {
        ctx.status("Set a Pulsoid widget ID in Settings");
        while !ctx.sleep(Duration::from_secs(3600)) {}
        return;
    }
    while !ctx.stopped() {
        match socket_url(widget) {
            Ok(url) => listen(ctx, &url),
            Err(error) => {
                let _ = crate::log::write_changed(&format!("pulsoid widget lookup: {error}"));
                ctx.status("Pulsoid widget not found, retrying");
            }
        }
        ctx.sleep(RETRY);
    }
}

fn socket_url(widget: &str) -> Result<String, String> {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
        .to_string();
    let body = serde_json::json!({
        "id": id,
        "jsonrpc": "2.0",
        "method": "getWidget",
        "params": { "widgetId": widget },
    });
    let text = crate::net::agent(Duration::from_secs(15))
        .post(RPC_URL)
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    ramiel_url(&text)
}

pub fn ramiel_url(reply: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(reply).map_err(|e| e.to_string())?;
    if let Some(error) = value.get("error") {
        return Err(error.to_string());
    }
    value["result"]["ramielUrl"]
        .as_str()
        .filter(|u| u.starts_with("wss://") || u.starts_with("ws://"))
        .map(str::to_owned)
        .ok_or_else(|| "reply has no socket address".into())
}

pub fn bpm_from_message(message: &str) -> Option<u16> {
    let value: Value = serde_json::from_str(message).ok()?;
    let rate = &value["data"]["heartRate"];
    let bpm = rate
        .as_f64()
        .or_else(|| rate.as_str()?.trim().parse().ok())?;
    heart_rate::parse_bpm_text(&bpm.to_string())
}

fn listen(ctx: &Context, url: &str) {
    let Ok(request) = tungstenite::client::IntoClientRequest::into_client_request(url) else {
        ctx.status("Pulsoid sent a bad socket address");
        return;
    };
    let host = request.uri().host().unwrap_or_default().to_owned();
    let port = request.uri().port_u16().unwrap_or(443);
    let tcp = match TcpStream::connect((host.as_str(), port)) {
        Ok(tcp) => tcp,
        Err(error) => {
            crate::log::write(&format!("pulsoid connect: {error}"));
            ctx.status("Can't reach Pulsoid, retrying");
            return;
        }
    };
    let _ = tcp.set_read_timeout(Some(SILENCE));
    if let Ok(clone) = tcp.try_clone() {
        ctx.set_wake(Wake::Shutdown(clone));
    }
    let mut socket = match tungstenite::client_tls(request, tcp) {
        Ok((socket, _)) => socket,
        Err(error) => {
            crate::log::write(&format!("pulsoid handshake: {error}"));
            ctx.status("Can't reach Pulsoid, retrying");
            return;
        }
    };
    ctx.status("Pulsoid connected");
    crate::log::write("pulsoid connected");
    while !ctx.stopped() {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Some(bpm) = bpm_from_message(&text) {
                    ctx.reading(bpm);
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(error) => {
                if !ctx.stopped() {
                    crate::log::write(&format!("pulsoid socket: {error}"));
                }
                break;
            }
        }
    }
    ctx.set_wake(Wake::None);
    if !ctx.stopped() {
        ctx.status("Pulsoid disconnected, retrying");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_reply() {
        let ok = r#"{"jsonrpc":"2.0","id":"1","result":{"ramielUrl":"wss://ramiel.pulsoid.net/listen/abc"}}"#;
        assert_eq!(
            ramiel_url(ok).unwrap(),
            "wss://ramiel.pulsoid.net/listen/abc"
        );
        let missing =
            r#"{"jsonrpc":"2.0","error":{"code":-33404,"message":"EntityNotFound"},"id":"1"}"#;
        assert!(ramiel_url(missing).is_err());
        assert!(ramiel_url(r#"{"result":{"ramielUrl":"http://x"}}"#).is_err());
        assert!(ramiel_url("<html>").is_err());
    }

    #[test]
    fn socket_messages() {
        assert_eq!(
            bpm_from_message(r#"{"timestamp":1,"data":{"heartRate":72}}"#),
            Some(72)
        );
        assert_eq!(bpm_from_message(r#"{"data":{"heartRate":"81"}}"#), Some(81));
        assert_eq!(bpm_from_message(r#"{"data":{"heartRate":0}}"#), None);
        assert_eq!(bpm_from_message(r#"{"data":{}}"#), None);
        assert_eq!(bpm_from_message("ping"), None);
    }

    #[test]
    #[ignore = "talks to api.stromno.com"]
    fn unknown_widget_is_reported_by_the_real_service() {
        let error = socket_url("00000000-0000-0000-0000-000000000000").unwrap_err();
        assert!(error.contains("EntityNotFound"), "{error}");
    }
}

use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

pub fn agent(timeout: Duration) -> ureq::Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(timeout))
        .user_agent(format!("heartwire/{}", crate::version::VERSION))
        .build()
        .into()
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::time::Duration;

    #[test]
    fn https_requests_reach_the_tls_layer_without_panicking() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                drop(stream);
            }
        });
        let result = super::agent(Duration::from_secs(5))
            .get(&format!("https://127.0.0.1:{port}/"))
            .call();
        assert!(result.is_err(), "a closed socket is not a TLS server");
        server.join().unwrap();
    }
}

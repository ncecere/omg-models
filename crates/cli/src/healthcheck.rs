//! `omg-models healthcheck`: a dependency-free HTTP/1.1 probe for distroless
//! images (no shell, curl or wget). Exits 0 only for a 2xx from `/healthz`
//! over loopback; ignores proxies and never follows redirects.

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    process::ExitCode,
    time::Duration,
};

use clap::Args;

#[derive(Args)]
pub struct HealthcheckArgs {
    /// The server's listen address; the probe connects to loopback on its port.
    #[arg(long, env = "OMG_MODELS_LISTEN", default_value = "127.0.0.1:8080")]
    pub listen: SocketAddr,
    /// Timeout in seconds for connect, write and read.
    #[arg(long, default_value_t = 3)]
    pub timeout: u64,
}

fn probe(addr: SocketAddr, timeout: Duration) -> Result<u16, String> {
    let mut stream =
        TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nUser-Agent: omg-models-healthcheck\r\n\r\n")
        .map_err(|e| format!("write: {e}"))?;
    let mut head = Vec::new();
    let mut buf = [0u8; 256];
    while !head.windows(2).any(|w| w == b"\r\n") && head.len() < 4096 {
        let n = stream.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            break;
        }
        head.extend_from_slice(&buf[..n]);
    }
    let line = String::from_utf8_lossy(&head);
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| {
            format!(
                "malformed response: {:?}",
                line.lines().next().unwrap_or("")
            )
        })?;
    Ok(status)
}

pub fn run(args: &HealthcheckArgs) -> ExitCode {
    let ip = if args.listen.ip().is_unspecified() || args.listen.ip().is_loopback() {
        if args.listen.is_ipv4() {
            std::net::IpAddr::from([127, 0, 0, 1])
        } else {
            std::net::IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1])
        }
    } else {
        args.listen.ip()
    };
    let addr = SocketAddr::new(ip, args.listen.port());
    match probe(addr, Duration::from_secs(args.timeout.max(1))) {
        Ok(status) if (200..300).contains(&status) => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!("unhealthy: /healthz returned {status}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("unhealthy: {error}");
            ExitCode::FAILURE
        }
    }
}

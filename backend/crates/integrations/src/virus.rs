//! Virus scanning of uploads with ClamAV's `clamd` (INSTREAM over TCP).

use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

const TIMEOUT: Duration = Duration::from_secs(30);
const CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    Infected(String),
    /// Scanning is disabled (local development only; refused by config elsewhere).
    NotScanned,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("virus scanner unavailable: {0}")]
    Unavailable(String),
}

pub enum VirusScanner {
    ClamAv(String),
    Disabled,
}

impl VirusScanner {
    pub async fn scan(&self, bytes: &[u8]) -> Result<Verdict, ScanError> {
        match self {
            VirusScanner::Disabled => Ok(Verdict::NotScanned),
            VirusScanner::ClamAv(addr) => tokio::time::timeout(TIMEOUT, clamd_instream(addr, bytes))
                .await
                .map_err(|_| ScanError::Unavailable("timed out".into()))?
                .map_err(|e| ScanError::Unavailable(e.to_string())),
        }
    }
}

async fn clamd_instream(addr: &str, bytes: &[u8]) -> std::io::Result<Verdict> {
    let mut s = TcpStream::connect(addr).await?;
    s.write_all(b"zINSTREAM\0").await?;
    for chunk in bytes.chunks(CHUNK) {
        s.write_all(&(chunk.len() as u32).to_be_bytes()).await?;
        s.write_all(chunk).await?;
    }
    s.write_all(&0u32.to_be_bytes()).await?;
    let mut reply = Vec::new();
    s.read_to_end(&mut reply).await?;
    Ok(parse_reply(&reply))
}

/// `stream: OK` or `stream: <signature> FOUND`. Anything else (e.g. `INSTREAM size limit
/// exceeded`) is treated as infected so a scanner problem never lets a file through.
fn parse_reply(raw: &[u8]) -> Verdict {
    let text = String::from_utf8_lossy(raw);
    let text = text.trim_end_matches(['\0', '\n']).trim();
    let body = text.strip_prefix("stream:").map(str::trim).unwrap_or(text);
    if body == "OK" {
        Verdict::Clean
    } else if let Some(sig) = body.strip_suffix("FOUND") {
        Verdict::Infected(sig.trim().chars().take(100).collect())
    } else {
        Verdict::Infected(format!("scanner error: {}", body.chars().take(100).collect::<String>()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clamd_replies() {
        assert_eq!(parse_reply(b"stream: OK\0"), Verdict::Clean);
        assert_eq!(parse_reply(b"stream: Eicar-Test-Signature FOUND\0"), Verdict::Infected("Eicar-Test-Signature".into()));
        assert!(matches!(parse_reply(b"INSTREAM size limit exceeded. ERROR\0"), Verdict::Infected(_)));
    }

    #[tokio::test]
    async fn unreachable_scanner_is_an_error_not_a_pass() {
        let s = VirusScanner::ClamAv("127.0.0.1:1".into());
        assert!(s.scan(b"x").await.is_err());
    }
}

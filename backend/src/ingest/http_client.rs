//! Shared HTTP client and bounded fetch (spec §3.1, §4.3, §38.1, §38.2).
//!
//! SSRF / DNS-rebinding control (Rev.2.9 §4.3): a custom resolver is invoked
//! by the connection pool for *every* connection attempt and drops every
//! prohibited address, so the client can only connect to vetted IPs while the
//! original hostname is kept for Host/SNI/certificate checks. Redirects are
//! limited to approved hosts and capped; proxies from the environment are
//! disabled; IP-literal URLs are checked explicitly.

use crate::ingest::source_registry::{SourceConfig, SourceFormat};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

pub const MAX_REDIRECTS: usize = 3;

pub fn ipv4_prohibited(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local() // includes 169.254.169.254 cloud metadata
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_documentation()
        || o[0] == 0 // "this network"
        || (o[0] == 100 && (o[1] & 0xC0) == 64) // 100.64/10 shared address space
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24 protocol assignments
        || (o[0] == 198 && (o[1] & 0xFE) == 18) // 198.18/15 benchmarking
        || o[0] >= 240 // 240/4 reserved
}

pub fn ipv6_prohibited(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return ipv4_prohibited(v4);
    }
    let s = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // fc00::/7 unique local
        || (s[0] & 0xffc0) == 0xfe80 // fe80::/10 link local
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || (s[0..6] == [0, 0, 0, 0, 0, 0]) // IPv4-compatible / reserved ::/96
        || (s[0] == 0x0064 && s[1] == 0xff9b) // NAT64 well-known prefix (may reach v4 internals)
}

pub fn ip_prohibited(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_prohibited(v4),
        IpAddr::V6(v6) => ipv6_prohibited(v6),
    }
}

/// Connection-time resolver that filters prohibited destinations.
pub struct SafeResolver {
    pub allow_private: bool,
}

impl Resolve for SafeResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let allow = self.allow_private;
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
                .collect();
            let ok: Vec<SocketAddr> = addrs.into_iter().filter(|a| allow || !ip_prohibited(a.ip())).collect();
            if ok.is_empty() {
                tracing::warn!(host = %host, "all resolved addresses prohibited by egress policy");
                return Err(format!("egress policy: no permitted address for {host}").into());
            }
            let it: Addrs = Box::new(ok.into_iter());
            Ok(it)
        })
    }
}

pub fn build_client(user_agent: &str, sources: &[SourceConfig], allow_private: bool) -> reqwest::Result<reqwest::Client> {
    let mut hosts: HashSet<String> = HashSet::new();
    for s in sources {
        if let Some(h) = s.feed_host() {
            hosts.insert(h.to_ascii_lowercase());
        }
        for h in &s.allowed_redirect_hosts {
            hosts.insert(h.to_ascii_lowercase());
        }
    }
    let hosts = Arc::new(hosts);
    let policy = reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        let host = attempt.url().host_str().map(|h| h.to_ascii_lowercase());
        match host {
            Some(h) if hosts.contains(&h) => attempt.follow(),
            _ => attempt.error("redirect outside approved hosts"),
        }
    });
    reqwest::Client::builder()
        .user_agent(user_agent)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(2)
        .redirect(policy)
        .no_proxy()
        .https_only(!allow_private)
        .dns_resolver(Arc::new(SafeResolver { allow_private }))
        .build()
}

#[derive(Debug)]
pub enum FetchOutcome {
    /// 304: checked and unchanged — healthy, but not new content.
    NotModified,
    Body {
        bytes: Vec<u8>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FetchError {
    #[error("timeout")]
    Timeout,
    #[error("dns/connect: {0}")]
    Connect(String),
    #[error("http status {code}")]
    Status { code: u16, retry_after_s: Option<u64> },
    #[error("response exceeds {0} bytes")]
    TooLarge(usize),
    #[error("redirect rejected: {0}")]
    Redirect(String),
    #[error("destination prohibited: {0}")]
    Prohibited(String),
    #[error("{0}")]
    Other(String),
}

impl FetchError {
    pub fn code(&self) -> String {
        match self {
            FetchError::Timeout => "timeout".into(),
            FetchError::Connect(_) => "connect".into(),
            FetchError::Status { code, .. } => format!("http_{code}"),
            FetchError::TooLarge(_) => "too_large".into(),
            FetchError::Redirect(_) => "redirect".into(),
            FetchError::Prohibited(_) => "prohibited".into(),
            FetchError::Other(_) => "other".into(),
        }
    }
    /// Permanent configuration errors must not be retried rapidly (spec §4.3).
    pub fn is_permanent(&self) -> bool {
        match self {
            FetchError::Status { code, .. } => (400..500).contains(code) && *code != 408 && *code != 429,
            FetchError::Redirect(_) | FetchError::Prohibited(_) => true,
            _ => false,
        }
    }
}

pub fn parse_retry_after(v: &str, now_ms: i64) -> Option<u64> {
    let v = v.trim();
    if let Ok(s) = v.parse::<u64>() {
        return Some(s);
    }
    chrono::DateTime::parse_from_rfc2822(v).ok().map(|d| ((d.timestamp_millis() - now_ms).max(0) / 1000) as u64)
}

fn classify(e: &reqwest::Error) -> FetchError {
    if e.is_timeout() {
        return FetchError::Timeout;
    }
    if e.is_redirect() {
        return FetchError::Redirect(e.to_string());
    }
    let mut msg = e.to_string();
    let mut src: Option<&dyn std::error::Error> = std::error::Error::source(e);
    while let Some(s) = src {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        src = s.source();
    }
    if msg.contains("egress policy") {
        return FetchError::Prohibited(msg);
    }
    if msg.contains("redirect") {
        return FetchError::Redirect(msg);
    }
    if e.is_connect() || e.is_request() {
        return FetchError::Connect(msg);
    }
    FetchError::Other(msg)
}

/// One bounded conditional fetch.
pub async fn fetch_once(
    client: &reqwest::Client,
    src: &SourceConfig,
    etag: Option<&str>,
    last_modified: Option<&str>,
    allow_private: bool,
) -> Result<FetchOutcome, FetchError> {
    let url = reqwest::Url::parse(&src.feed_url).map_err(|e| FetchError::Other(e.to_string()))?;
    // IP-literal hosts bypass the resolver; check them explicitly.
    if let Some(h) = url.host_str() {
        let h = h.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = h.parse::<IpAddr>() {
            if !allow_private && ip_prohibited(ip) {
                return Err(FetchError::Prohibited(ip.to_string()));
            }
        }
    }
    let accept = match src.format {
        SourceFormat::Cap => "application/cap+xml, application/xml;q=0.9, text/xml;q=0.8",
        SourceFormat::GeojsonUsgs => "application/geo+json, application/json;q=0.9",
    };
    let mut req = client.get(url).timeout(Duration::from_secs(src.request_timeout_seconds)).header(reqwest::header::ACCEPT, accept);
    if let Some(e) = etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, e);
    }
    if let Some(lm) = last_modified {
        req = req.header(reqwest::header::IF_MODIFIED_SINCE, lm);
    }
    let mut resp = req.send().await.map_err(|e| classify(&e))?;
    // Final host must still be approved for *this* source.
    let final_host = resp.url().host_str().map(|h| h.to_ascii_lowercase());
    let feed_host = src.feed_host().map(|h| h.to_ascii_lowercase());
    let approved = final_host.is_some()
        && (final_host == feed_host || src.allowed_redirect_hosts.iter().any(|h| Some(h.to_ascii_lowercase()) == final_host));
    if !approved {
        return Err(FetchError::Redirect(format!("final host {final_host:?} not approved for source")));
    }
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(FetchOutcome::NotModified);
    }
    if !status.is_success() {
        let retry_after_s = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| parse_retry_after(v, crate::domain::now_ms()));
        return Err(FetchError::Status { code: status.as_u16(), retry_after_s });
    }
    let header = |n: reqwest::header::HeaderName| resp.headers().get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
    let etag = header(reqwest::header::ETAG);
    let last_modified = header(reqwest::header::LAST_MODIFIED);
    if let Some(len) = resp.content_length() {
        if len as usize > src.max_response_bytes {
            return Err(FetchError::TooLarge(src.max_response_bytes));
        }
    }
    // Stream and stop as soon as the decoded-byte limit is exceeded.
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| classify(&e))? {
        if bytes.len() + chunk.len() > src.max_response_bytes {
            return Err(FetchError::TooLarge(src.max_response_bytes));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(FetchOutcome::Body { bytes, etag, last_modified })
}

//! Edge middleware run by the Rust server: security headers, CORS for an
//! explicitly configured frontend origin, and per-IP rate limiting.

use crate::config::HttpConfig;
use crate::error::ApiError;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Content Security Policy for the frontend. Map tiles, styles, glyphs and
/// sprites come from OpenFreeMap; the MapLibre library from unpkg (pinned version).
pub const CSP: &str = "default-src 'self'; \
script-src 'self' https://unpkg.com; \
style-src 'self' 'unsafe-inline' https://unpkg.com; \
img-src 'self' data: blob: https://tiles.openfreemap.org; \
font-src 'self'; \
connect-src 'self' https://tiles.openfreemap.org; \
worker-src 'self' blob:; \
child-src blob:; \
manifest-src 'self'; \
object-src 'none'; \
base-uri 'self'; \
form-action 'self'; \
frame-ancestors 'none'";

/// Process-wide counters exposed at `/health/metrics` (Prometheus text format).
#[derive(Default)]
pub struct Metrics {
    pub api_requests: AtomicU64,
    pub api_errors_5xx: AtomicU64,
    pub api_errors_4xx: AtomicU64,
    pub rate_limited: AtomicU64,
}

impl Metrics {
    pub fn get(c: &AtomicU64) -> u64 {
        c.load(Ordering::Relaxed)
    }
}

pub struct Edge {
    pub cfg: HttpConfig,
    pub limiter: RateLimiter,
    pub metrics: Arc<Metrics>,
}

impl Edge {
    pub fn new(cfg: HttpConfig, metrics: Arc<Metrics>) -> Self {
        let limiter = RateLimiter::new(cfg.rate_limit_per_minute);
        Self { cfg, limiter, metrics }
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        self.cfg.cors_allowed_origins.iter().any(|o| o == "*" || o == origin)
    }
}

/// Fixed one-minute window counter per client IP.
pub struct RateLimiter {
    per_minute: u32,
    windows: Mutex<HashMap<IpAddr, (u64, u32)>>,
}

const MAX_TRACKED: usize = 100_000;

impl RateLimiter {
    pub fn new(per_minute: u32) -> Self {
        Self { per_minute, windows: Mutex::new(HashMap::new()) }
    }

    /// Returns `Err(seconds_until_reset)` when the client is over the limit.
    pub fn check(&self, ip: IpAddr, now_ms: i64) -> Result<(), u64> {
        if self.per_minute == 0 {
            return Ok(());
        }
        let window = (now_ms.max(0) as u64) / 60_000;
        let mut m = match self.windows.lock() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        };
        if m.len() > MAX_TRACKED {
            m.retain(|_, (w, _)| *w == window);
        }
        let e = m.entry(ip).or_insert((window, 0));
        if e.0 != window {
            *e = (window, 0);
        }
        if e.1 >= self.per_minute {
            let reset = 60 - ((now_ms.max(0) as u64) / 1000) % 60;
            return Err(reset);
        }
        e.1 += 1;
        Ok(())
    }
}

/// Client IP: the socket peer, or the right-most `X-Forwarded-For` hop when
/// the server is configured to sit behind a trusted reverse proxy.
pub fn client_ip(req: &Request, trust_forwarded_for: bool) -> Option<IpAddr> {
    if trust_forwarded_for {
        if let Some(ip) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .and_then(|s| s.trim().parse::<IpAddr>().ok())
        {
            return Some(ip);
        }
    }
    req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip())
}

fn set(res: &mut Response, name: HeaderName, value: &'static str) {
    res.headers_mut().insert(name, HeaderValue::from_static(value));
}

pub async fn edge(State(e): State<Arc<Edge>>, req: Request, next: Next) -> Response {
    let origin = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()).map(str::to_string);
    let cors_origin = origin.filter(|o| e.origin_allowed(o));
    let is_api = req.uri().path().starts_with("/api/");

    // CORS preflight for the API (GET only).
    if req.method() == Method::OPTIONS && is_api {
        let mut res = StatusCode::NO_CONTENT.into_response();
        if let Some(o) = &cors_origin {
            apply_cors(&mut res, o);
            set(&mut res, header::ACCESS_CONTROL_ALLOW_METHODS, "GET, OPTIONS");
            set(&mut res, header::ACCESS_CONTROL_ALLOW_HEADERS, "accept, last-event-id, cache-control");
            set(&mut res, header::ACCESS_CONTROL_MAX_AGE, "600");
        }
        security_headers(&mut res, e.cfg.hsts);
        return res;
    }

    if is_api {
        e.metrics.api_requests.fetch_add(1, Ordering::Relaxed);
        if let Some(ip) = client_ip(&req, e.cfg.trust_forwarded_for) {
            if let Err(retry) = e.limiter.check(ip, crate::domain::now_ms()) {
                e.metrics.rate_limited.fetch_add(1, Ordering::Relaxed);
                let mut res = ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "Too many requests. Please wait and try again.")
                    .into_response();
                if let Ok(v) = HeaderValue::from_str(&retry.to_string()) {
                    res.headers_mut().insert(header::RETRY_AFTER, v);
                }
                if let Some(o) = &cors_origin {
                    apply_cors(&mut res, o);
                }
                security_headers(&mut res, e.cfg.hsts);
                return res;
            }
        }
        if !matches!(*req.method(), Method::GET | Method::HEAD) {
            let mut res = ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "Only GET is supported.").into_response();
            security_headers(&mut res, e.cfg.hsts);
            return res;
        }
    }

    let mut res = next.run(req).await;
    if is_api {
        let code = res.status().as_u16();
        if code >= 500 {
            e.metrics.api_errors_5xx.fetch_add(1, Ordering::Relaxed);
        } else if code >= 400 {
            e.metrics.api_errors_4xx.fetch_add(1, Ordering::Relaxed);
        }
    }
    if let Some(o) = &cors_origin {
        apply_cors(&mut res, o);
    }
    if is_api && !res.headers().contains_key(header::CACHE_CONTROL) {
        set(&mut res, header::CACHE_CONTROL, "no-store");
    }
    security_headers(&mut res, e.cfg.hsts);
    res
}

fn apply_cors(res: &mut Response, origin: &str) {
    if let Ok(v) = HeaderValue::from_str(origin) {
        res.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
    }
    res.headers_mut().append(header::VARY, HeaderValue::from_static("Origin"));
}

pub fn security_headers(res: &mut Response, hsts: bool) {
    set(res, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(res, header::REFERRER_POLICY, "strict-origin-when-cross-origin");
    set(res, header::X_FRAME_OPTIONS, "DENY");
    set(res, HeaderName::from_static("permissions-policy"), "geolocation=(self), camera=(), microphone=(), payment=()");
    set(res, HeaderName::from_static("cross-origin-opener-policy"), "same-origin");
    set(res, header::CONTENT_SECURITY_POLICY, CSP);
    if hsts {
        set(res, header::STRICT_TRANSPORT_SECURITY, "max-age=31536000; includeSubDomains");
    }
}

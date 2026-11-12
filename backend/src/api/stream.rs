//! `/api/v1/stream` — SSE with durable replay (spec §10, §25, §41.6, §41.7).

use crate::error::ApiError;
use crate::realtime::event::STREAM_RESET;
use crate::realtime::replay::{start, ReplayConfig, StreamItem};
use crate::state::AppState;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{header, HeaderMap, HeaderName, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

/// Releases the per-IP slot when the stream ends.
struct IpGuard {
    st: Arc<crate::state::SseLimits>,
    ip: IpAddr,
}
impl Drop for IpGuard {
    fn drop(&mut self) {
        if let Ok(mut m) = self.st.per_ip.lock() {
            if let Some(n) = m.get_mut(&self.ip) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    m.remove(&self.ip);
                }
            }
        }
    }
}

pub fn parse_cursor(headers: &HeaderMap, q: &HashMap<String, String>) -> Result<u64, ApiError> {
    // Prefer the standard Last-Event-ID on browser reconnects.
    if let Some(v) = headers.get("last-event-id") {
        let s = v.to_str().map_err(|_| ApiError::bad_request("invalid_cursor", "Last-Event-ID is invalid."))?;
        return s.trim().parse().map_err(|_| ApiError::bad_request("invalid_cursor", "Last-Event-ID must be an integer."));
    }
    match q.get("after") {
        Some(a) => a.parse().map_err(|_| ApiError::bad_request("invalid_cursor", "after must be a non-negative integer.")),
        None => Ok(0),
    }
}

fn to_event(item: StreamItem) -> Event {
    match item {
        StreamItem::Event(e) => {
            let ev = Event::default().id(e.event_sequence.to_string()).event(e.event_type.clone());
            ev.json_data(&e).unwrap_or_else(|_| Event::default().comment("serialization error"))
        }
        StreamItem::Reset { reason, high_water } => Event::default()
            .event(STREAM_RESET)
            .data(serde_json::json!({ "reason": reason, "high_water": high_water, "action": "fetch_snapshot" }).to_string()),
    }
}

pub async fn stream(
    State(st): State<AppState>,
    connect: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    let after = parse_cursor(&headers, &q)?;
    let ip = connect.map(|c| c.0.ip()).unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let permit = st
        .sse
        .total
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "stream_capacity", "Too many live connections; retry later."))?;
    {
        let mut m = st.sse.per_ip.lock().map_err(|_| ApiError::internal())?;
        let n = m.entry(ip).or_insert(0);
        if *n >= st.sse.max_per_ip {
            return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "stream_limit", "Too many live connections from this client."));
        }
        *n += 1;
    }
    let guard = IpGuard { st: st.sse.clone(), ip };
    let rx = start(
        st.db.clone(),
        &st.bus,
        after,
        ReplayConfig { max_replay_events: st.api.max_replay_events, client_queue: st.api.client_queue },
    );
    let hello = tokio_stream::once(Event::default().retry(Duration::from_secs(5)).comment("connected"));
    let body = hello
        .chain(ReceiverStream::new(rx).map(move |item| {
            let _held = (&permit, &guard); // released when the client disconnects
            to_event(item)
        }))
        .map(Ok::<Event, Infallible>);
    let sse = Sse::new(body).keep_alive(KeepAlive::new().interval(Duration::from_secs(st.api.heartbeat_seconds)).text("hb"));
    Ok((
        [
            (header::CONTENT_TYPE, "text/event-stream; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache, no-transform"),
            (HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        sse,
    ))
}

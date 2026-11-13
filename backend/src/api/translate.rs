//! Machine translation of an alert's text through a LibreTranslate-compatible
//! service, called only by this server. Disabled unless configured; the API key
//! (if the service needs one) is read from the environment, never from files or
//! the frontend. Results are labelled as machine translation and the original
//! wording is always returned alongside. Nothing is translated automatically.

use crate::config::TranslationConfig;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Translator {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    max_chars: usize,
    provider_name: String,
    gate: tokio::sync::Semaphore,
    cache: Mutex<HashMap<String, Value>>,
}

/// Target languages accepted (ISO 639-1); the provider may support fewer.
pub fn valid_lang(l: &str) -> bool {
    (2..=3).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_lowercase())
}

impl Translator {
    pub fn new(cfg: &TranslationConfig, user_agent: &str) -> reqwest::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .https_only(cfg.base_url.starts_with("https://"))
            .build()?;
        Ok(Self {
            client,
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            api_key: std::env::var("WDA_TRANSLATE_API_KEY").ok().filter(|k| !k.is_empty()),
            max_chars: cfg.max_chars,
            provider_name: cfg.provider_name.clone(),
            gate: tokio::sync::Semaphore::new(2),
            cache: Mutex::new(HashMap::new()),
        })
    }

    async fn translate(&self, texts: &[String], target: &str) -> Result<Vec<String>, ApiError> {
        let unavailable =
            || ApiError::new(StatusCode::BAD_GATEWAY, "translation_failed", "Machine translation is temporarily unavailable.");
        let _permit = tokio::time::timeout(Duration::from_secs(5), self.gate.acquire())
            .await
            .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "translation_busy", "Translation is busy. Try again shortly."))?
            .map_err(|_| unavailable())?;
        let mut body = json!({ "q": texts, "source": "auto", "target": target, "format": "text" });
        if let Some(k) = &self.api_key {
            body["api_key"] = Value::String(k.clone());
        }
        let res = self.client.post(format!("{}/translate", self.base_url)).json(&body).send().await.map_err(|e| {
            tracing::warn!(error = %e, "translation request failed");
            unavailable()
        })?;
        if !res.status().is_success() {
            tracing::warn!(status = res.status().as_u16(), "translation provider error");
            return Err(unavailable());
        }
        let v: Value = res.json().await.map_err(|_| unavailable())?;
        let out: Vec<String> = match v.get("translatedText") {
            Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            Some(Value::String(s)) if texts.len() == 1 => vec![s.clone()],
            _ => return Err(unavailable()),
        };
        if out.len() != texts.len() {
            return Err(unavailable());
        }
        Ok(out)
    }
}

pub async fn translate_alert(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    if id.len() > 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::bad_request("invalid_alert_id", "Alert id is invalid."));
    }
    let lang = q.get("lang").map(String::as_str).unwrap_or("");
    if !valid_lang(lang) {
        return Err(ApiError::bad_request("invalid_lang", "lang must be an ISO 639-1 code."));
    }
    let Some(tr) = st.translator.clone() else {
        return Err(ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            "translation_unavailable",
            "Machine translation is not enabled on this server.",
        ));
    };
    let rec = { st.index.read().map_err(|_| ApiError::internal())?.alerts.get(&id).cloned() }
        .ok_or_else(|| ApiError::not_found("This alert was not found or is no longer available."))?;
    let key = format!("{}:{}:{lang}", rec.alert_id, rec.revision);
    if let Some(v) = tr.cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(Json(v));
    }
    let v0 = rec.language_variants.first().ok_or_else(|| ApiError::not_found("This alert has no text to translate."))?;
    if v0.language.to_ascii_lowercase().starts_with(lang) {
        return Err(ApiError::bad_request("same_language", "The alert is already in this language."));
    }
    let fields: Vec<(&str, String)> = [("headline", &v0.headline), ("description", &v0.description), ("instruction", &v0.instruction)]
        .into_iter()
        .filter_map(|(k, v)| v.clone().filter(|t| !t.trim().is_empty()).map(|t| (k, t)))
        .collect();
    let total: usize = fields.iter().map(|(_, t)| t.chars().count()).sum();
    if fields.is_empty() || total > tr.max_chars {
        return Err(ApiError::bad_request("too_long", "This alert text cannot be machine translated here."));
    }
    let texts: Vec<String> = fields.iter().map(|(_, t)| t.clone()).collect();
    let out = tr.translate(&texts, lang).await?;
    let mut items = serde_json::Map::new();
    let mut original = serde_json::Map::new();
    for ((k, orig), t) in fields.iter().zip(out) {
        items.insert((*k).to_string(), Value::String(t));
        original.insert((*k).to_string(), Value::String(orig.clone()));
    }
    let v = json!({
        "status": "machine_translation",
        "notice": "Machine translation. It may contain errors. The original official text takes priority.",
        "provider": tr.provider_name,
        "alert_id": rec.alert_id,
        "revision": rec.revision,
        "source_language": v0.language,
        "target_language": lang,
        "items": items,
        "original": original,
    });
    if let Ok(mut c) = tr.cache.lock() {
        if c.len() > 2000 {
            c.clear();
        }
        c.insert(key, v.clone());
    }
    Ok(Json(v))
}

pub type SharedTranslator = Option<Arc<Translator>>;

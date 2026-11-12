//! Mandatory source registry (spec §4.1, §4.6) and runtime health (§4.1, §77.5).
//! `coverage_state`, `freshness_state` and `onboarding_state` stay independent.

use crate::config::Environment;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
pub struct SourcesFile {
    #[serde(default)]
    pub sources: Vec<SourceConfig>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat {
    Cap,
    GeojsonUsgs,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceConfig {
    // ---- core fields (spec §4.1) ----
    pub source_id: String,
    pub agency_name: String,
    pub country_or_region: String,
    pub official_homepage: String,
    pub feed_url: String,
    pub format: SourceFormat,
    pub hazard_types: Vec<String>,
    pub geographic_coverage: String,
    pub poll_interval_seconds: u64,
    pub request_timeout_seconds: u64,
    pub max_response_bytes: usize,
    pub expected_freshness_seconds: u64,
    pub terms_or_redistribution_notes: String,
    pub adapter_name: String,
    pub enabled: bool,
    // ---- onboarding / licensing fields (spec §4.6) ----
    pub publisher_class: String,
    pub authority_role: String,
    #[serde(default)]
    pub jurisdiction_id: Option<String>,
    #[serde(default)]
    pub jurisdiction_level: Option<String>,
    #[serde(default)]
    pub source_language: Vec<String>,
    #[serde(default)]
    pub source_timezone: Option<String>,
    #[serde(default = "pull")]
    pub update_delivery_model: String,
    #[serde(default)]
    pub cadence_evidence_url: Option<String>,
    #[serde(default)]
    pub observed_cadence_seconds: Option<u64>,
    #[serde(default)]
    pub cache_policy: Option<String>,
    #[serde(default = "none")]
    pub authentication: String,
    #[serde(default)]
    pub rate_limit_policy: Option<String>,
    #[serde(default)]
    pub terms_url: Option<String>,
    #[serde(default)]
    pub terms_version_or_checked_date: Option<String>,
    pub redistribution_status: String,
    #[serde(default = "unknown")]
    pub commercial_use_allowed: String,
    #[serde(default = "unknown")]
    pub adaptation_allowed: String,
    #[serde(default)]
    pub attribution_template: Option<String>,
    #[serde(default)]
    pub retention_restrictions: Option<String>,
    #[serde(default)]
    pub source_contact: Option<String>,
    #[serde(default)]
    pub verification_evidence_url: Vec<String>,
    #[serde(default)]
    pub verification_reviewer: Option<String>,
    #[serde(default)]
    pub verification_reviewed_at: Option<String>,
    #[serde(default)]
    pub review_due_at: Option<String>,
    #[serde(default)]
    pub adapter_version: Option<String>,
    #[serde(default)]
    pub fallback_source_ids: Vec<String>,
    pub coverage_state: String,
    pub onboarding_state: String,
    /// Extra hosts a redirect may reach (exact host match). Feed host is always allowed.
    #[serde(default)]
    pub allowed_redirect_hosts: Vec<String>,
}

fn pull() -> String {
    "pull".into()
}
fn none() -> String {
    "none".into()
}
fn unknown() -> String {
    "unknown".into()
}

const PUBLIC_OK: &[&str] = &["approved", "attribution_required"];

impl SourceConfig {
    pub fn feed_host(&self) -> Option<String> {
        reqwest::Url::parse(&self.feed_url).ok().and_then(|u| u.host_str().map(str::to_string))
    }

    /// Public redistribution is allowed only after onboarding approval and an
    /// approved rights status (spec §4.6 rule 5).
    pub fn public_redistribution(&self) -> bool {
        self.onboarding_state == "approved" && PUBLIC_OK.contains(&self.redistribution_status.as_str())
    }

    pub fn validate(&self, env: Environment, allow_private: bool) -> Result<(), String> {
        let url = reqwest::Url::parse(&self.feed_url).map_err(|e| format!("feed_url: {e}"))?;
        if url.scheme() != "https" && !(allow_private && url.scheme() == "http") {
            return Err("feed_url must use https".into());
        }
        if url.host_str().is_none() {
            return Err("feed_url has no host".into());
        }
        match (self.format, self.adapter_name.as_str()) {
            (SourceFormat::Cap, "cap") | (SourceFormat::GeojsonUsgs, "usgs_geojson") => {}
            _ => return Err(format!("adapter '{}' does not match format", self.adapter_name)),
        }
        if self.poll_interval_seconds < 15 {
            return Err("poll_interval_seconds must be >= 15".into());
        }
        if self.request_timeout_seconds == 0 || self.request_timeout_seconds > 120 {
            return Err("request_timeout_seconds must be 1..=120".into());
        }
        if self.max_response_bytes == 0 || self.max_response_bytes > 64 * 1024 * 1024 {
            return Err("max_response_bytes must be 1..=64MiB".into());
        }
        if self.expected_freshness_seconds == 0 {
            return Err("expected_freshness_seconds must be > 0".into());
        }
        if env == Environment::Production && self.enabled && !self.public_redistribution() {
            return Err(format!(
                "enabled in production but onboarding_state={} redistribution_status={} (requires approved review)",
                self.onboarding_state, self.redistribution_status
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessState {
    Fresh,
    Delayed,
    Stale,
    Unavailable,
    Unknown,
}

/// Runtime source-health state (spec §4.1). Persisted in `source_state`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceHealth {
    pub last_attempt_at_ms: Option<i64>,
    pub last_http_success_at_ms: Option<i64>,
    pub last_content_change_at_ms: Option<i64>,
    pub last_valid_record_at_ms: Option<i64>,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub consecutive_failures: u32,
    pub freshness_state: Option<FreshnessState>,
    pub last_content_hash: Option<String>,
    pub rejected_records_last_fetch: u32,
}

impl SourceHealth {
    pub fn freshness(&self) -> FreshnessState {
        self.freshness_state.unwrap_or(FreshnessState::Unknown)
    }
}

/// Deterministic freshness from the last successful check (spec §4.1, §77.5).
/// A 304 counts as a successful check, not as new content.
pub fn compute_freshness(h: &SourceHealth, expected_s: u64, now_ms: i64) -> FreshnessState {
    let Some(ok) = h.last_http_success_at_ms else {
        return if h.consecutive_failures > 0 { FreshnessState::Unavailable } else { FreshnessState::Unknown };
    };
    let age = (now_ms - ok).max(0) as u64;
    let exp = expected_s * 1000;
    if age <= exp {
        FreshnessState::Fresh
    } else if age <= exp * 2 {
        FreshnessState::Delayed
    } else if h.consecutive_failures >= 5 {
        FreshnessState::Unavailable
    } else {
        FreshnessState::Stale
    }
}

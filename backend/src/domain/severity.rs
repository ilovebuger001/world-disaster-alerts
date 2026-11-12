//! Versioned, deterministic display mapping (spec §7):
//! `SourceAlertValues -> SourcePolicy(source_id, hazard_type, mapping_version) -> DisplayPriority`.
//! AI never participates (spec §1.2 rule 3).

use super::alert::DisplayPriority;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct SeverityMapping {
    pub mapping_version: String,
    /// Rules evaluated in order; first match wins.
    pub rules: Vec<MappingRule>,
    /// Keyword rules mapping a source event/category text to a hazard display group.
    #[serde(default)]
    pub hazard_rules: Vec<HazardRule>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MappingRule {
    /// Restrict to a source id (None = any).
    pub source_id: Option<String>,
    /// Restrict to a normalized hazard type (None = any).
    pub hazard_type: Option<String>,
    pub severity: Option<String>,
    pub urgency: Option<String>,
    pub certainty: Option<String>,
    pub priority: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HazardRule {
    /// Case-insensitive substrings matched against CAP event text / categories.
    pub any_of: Vec<String>,
    pub hazard_type: String,
}

#[derive(Debug, thiserror::Error)]
pub enum MappingError {
    #[error("unknown display priority '{0}' in mapping rule")]
    UnknownPriority(String),
    #[error("mapping has no rules")]
    Empty,
}

pub struct MapInput<'a> {
    pub source_id: &'a str,
    pub hazard_type: &'a str,
    pub severity: Option<&'a str>,
    pub urgency: Option<&'a str>,
    pub certainty: Option<&'a str>,
}

impl SeverityMapping {
    pub fn validate(&self) -> Result<(), MappingError> {
        if self.rules.is_empty() {
            return Err(MappingError::Empty);
        }
        for r in &self.rules {
            if DisplayPriority::parse(&r.priority).is_none() {
                return Err(MappingError::UnknownPriority(r.priority.clone()));
            }
        }
        Ok(())
    }

    pub fn priority(&self, i: &MapInput) -> DisplayPriority {
        fn ok(rule: &Option<String>, v: Option<&str>) -> bool {
            match rule {
                None => true,
                Some(r) => v.is_some_and(|v| v.eq_ignore_ascii_case(r)),
            }
        }
        for r in &self.rules {
            let src = r.source_id.as_deref().map_or(true, |s| s == i.source_id);
            let hz = r.hazard_type.as_deref().map_or(true, |h| h == i.hazard_type);
            if src && hz && ok(&r.severity, i.severity) && ok(&r.urgency, i.urgency) && ok(&r.certainty, i.certainty) {
                return DisplayPriority::parse(&r.priority).unwrap_or(DisplayPriority::Unknown);
            }
        }
        DisplayPriority::Unknown
    }

    /// Returns the normalized hazard group, or "other" when no rule matches.
    pub fn hazard(&self, texts: &[&str]) -> String {
        for r in &self.hazard_rules {
            for t in texts {
                let lt = t.to_lowercase();
                if r.any_of.iter().any(|k| lt.contains(&k.to_lowercase())) {
                    return r.hazard_type.clone();
                }
            }
        }
        "other".to_string()
    }
}

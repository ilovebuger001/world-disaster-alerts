//! Reference datasets served by the Rust backend: the emergency contact
//! directory, preparedness guides and event-linked facilities.
//!
//! All three are loaded from versioned JSON files at startup and validated
//! before the server binds a port. Nothing here is invented at runtime: a
//! country without a verified number is reported as such, a hazard without a
//! reviewed guide is reported as unavailable, and a facility is only returned
//! when its record names the selected event.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ReferenceError {
    #[error("cannot read {0}: {1}")]
    Read(String, std::io::Error),
    #[error("invalid JSON in {0}: {1}")]
    Json(String, serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

// ---------------------------------------------------------------- contacts

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContactVerification {
    /// Confirmed on the responsible authority's own publication.
    Verified,
    /// Published by another government (e.g. travel advice), not the country itself.
    OfficialSecondary,
    Unverified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Verified,
    PartiallyVerified,
    Unverified,
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
    pub service: String,
    /// Human-readable form, shown to the user.
    pub display: String,
    /// Digits (optionally a leading +) used to build the `tel:` URI.
    pub dial: String,
    pub authority: String,
    pub source_url: String,
    pub verification: ContactVerification,
    pub last_verified: Option<String>,
    pub region: Option<String>,
    pub network: Option<String>,
    pub instructions: Option<String>,
    pub language: Option<String>,
    pub notes: Option<String>,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Country {
    pub code: String,
    pub alpha_3: Option<String>,
    pub numeric: Option<String>,
    pub name: String,
    pub official_name: Option<String>,
    pub coverage: Coverage,
    pub last_reviewed: Option<String>,
    pub contacts: Vec<Contact>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContactsFile {
    pub schema_version: u32,
    pub dataset_version: String,
    #[serde(default)]
    pub country_list_source: Option<serde_json::Value>,
    #[serde(default)]
    pub verification_levels: Option<serde_json::Value>,
    pub countries: Vec<Country>,
}

/// Valid dial string: 2–15 digits with an optional leading `+`.
pub fn valid_dial(d: &str) -> bool {
    let digits = d.strip_prefix('+').unwrap_or(d);
    (2..=15).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Builds a `tel:` URI for a contact. Returns `None` for anything that is not a
/// plain dial string, so the UI never links an unchecked value.
pub fn tel_uri(c: &Contact) -> Option<String> {
    valid_dial(&c.dial).then(|| format!("tel:{}", c.dial))
}

fn https(u: &str) -> bool {
    u.starts_with("https://") && u.len() > 12 && !u.contains(char::is_whitespace)
}

impl ContactsFile {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        let mut seen = HashSet::new();
        for c in &self.countries {
            let bad = |m: &str| ReferenceError::Invalid(format!("contacts {}: {m}", c.code));
            if c.code.len() != 2 || !c.code.bytes().all(|b| b.is_ascii_uppercase()) {
                return Err(bad("country code must be ISO 3166-1 alpha-2"));
            }
            if !seen.insert(c.code.clone()) {
                return Err(bad("duplicate country"));
            }
            for k in &c.contacts {
                if !valid_dial(&k.dial) {
                    return Err(bad(&format!("invalid dial string '{}'", k.dial)));
                }
                if k.display.trim().is_empty() || k.authority.trim().is_empty() {
                    return Err(bad("contact needs a display number and an authority"));
                }
                if !https(&k.source_url) {
                    return Err(bad("contact source_url must be https"));
                }
                if k.verification == ContactVerification::Verified && k.last_verified.is_none() {
                    return Err(bad("verified contact without last_verified date"));
                }
            }
            let any_checked = c.contacts.iter().any(|k| k.verification != ContactVerification::Unverified);
            match c.coverage {
                Coverage::Unavailable if !c.contacts.is_empty() => return Err(bad("coverage unavailable but contacts listed")),
                Coverage::Verified
                    if c.contacts.is_empty() || c.contacts.iter().any(|k| k.verification != ContactVerification::Verified) =>
                {
                    return Err(bad("coverage verified requires every contact verified"))
                }
                Coverage::PartiallyVerified if !any_checked => {
                    return Err(bad("partially verified requires an officially sourced contact"))
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn country(&self, code: &str) -> Option<&Country> {
        let code = code.to_ascii_uppercase();
        self.countries.iter().find(|c| c.code == code)
    }
}

// ------------------------------------------------------------------ guides

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Guide {
    pub hazard: String,
    pub title: String,
    pub authority: String,
    pub jurisdiction: String,
    pub scope: String,
    pub language: String,
    pub source_url: String,
    pub published_or_reviewed: Option<String>,
    pub retrieved: String,
    pub before: Vec<String>,
    pub during: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnavailableGuide {
    pub hazard: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GuidesFile {
    pub schema_version: u32,
    pub dataset_version: String,
    pub notice: String,
    pub guides: Vec<Guide>,
    #[serde(default)]
    pub unavailable: Vec<UnavailableGuide>,
}

impl GuidesFile {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        let mut seen = HashSet::new();
        for g in &self.guides {
            let bad = |m: &str| ReferenceError::Invalid(format!("guide {}: {m}", g.hazard));
            if !seen.insert(g.hazard.clone()) {
                return Err(bad("duplicate hazard"));
            }
            if !https(&g.source_url) || g.authority.trim().is_empty() {
                return Err(bad("guide needs an authority and an https source_url"));
            }
            if g.before.is_empty() && g.during.is_empty() && g.after.is_empty() {
                return Err(bad("guide has no content"));
            }
        }
        for u in &self.unavailable {
            if seen.contains(&u.hazard) {
                return Err(ReferenceError::Invalid(format!("guide {} is both available and unavailable", u.hazard)));
            }
        }
        Ok(())
    }

    pub fn guide(&self, hazard: &str) -> Option<&Guide> {
        self.guides.iter().find(|g| g.hazard == hazard)
    }
}

// -------------------------------------------------------------- facilities

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacilityStatus {
    /// Officially designated for this event.
    Designated,
    /// Officially confirmed open/available for this event.
    ConfirmedAvailable,
    /// On an official list for this event but availability not confirmed.
    ListedNotConfirmed,
    Unverified,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventLink {
    /// Matches `AlertRecord::alert_id`.
    #[serde(default)]
    pub alert_id: Option<String>,
    /// Or the pair (source_id, source_record_id) published by the source.
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub source_record_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Facility {
    pub facility_id: String,
    pub name: String,
    pub kind: String,
    pub lat: f64,
    pub lon: f64,
    pub status: FacilityStatus,
    pub authority: String,
    pub source_url: String,
    pub last_verified: Option<String>,
    /// Unix ms after which the record must not be shown.
    #[serde(default)]
    pub valid_until_ms: Option<i64>,
    #[serde(default)]
    pub capacity: Option<u32>,
    #[serde(default)]
    pub notes: Option<String>,
    pub linked_events: Vec<EventLink>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FacilitiesFile {
    pub schema_version: u32,
    pub dataset_version: String,
    pub notice: String,
    pub facilities: Vec<Facility>,
}

pub const FACILITY_KINDS: &[&str] = &["shelter", "evacuation_point", "hospital", "relief_point", "medical_post"];

impl FacilitiesFile {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        let mut seen = HashSet::new();
        for f in &self.facilities {
            let bad = |m: &str| ReferenceError::Invalid(format!("facility {}: {m}", f.facility_id));
            if !seen.insert(f.facility_id.clone()) {
                return Err(bad("duplicate facility_id"));
            }
            if !(-90.0..=90.0).contains(&f.lat) || !(-180.0..=180.0).contains(&f.lon) || !f.lat.is_finite() || !f.lon.is_finite() {
                return Err(bad("coordinates out of range"));
            }
            if !FACILITY_KINDS.contains(&f.kind.as_str()) {
                return Err(bad("unknown kind"));
            }
            if !https(&f.source_url) || f.authority.trim().is_empty() {
                return Err(bad("needs an authority and an https source_url"));
            }
            if f.linked_events.is_empty() {
                return Err(bad("a facility must be linked to at least one official event"));
            }
            for l in &f.linked_events {
                if l.alert_id.is_none() && (l.source_id.is_none() || l.source_record_id.is_none()) {
                    return Err(bad("event link needs alert_id or source_id + source_record_id"));
                }
            }
        }
        Ok(())
    }

    /// Facilities officially linked to this alert and still valid at `now_ms`.
    pub fn for_event<'a>(&'a self, alert_id: &str, source_id: &str, source_record_id: &str, now_ms: i64) -> Vec<&'a Facility> {
        self.facilities
            .iter()
            .filter(|f| f.valid_until_ms.map_or(true, |t| t > now_ms))
            .filter(|f| {
                f.linked_events.iter().any(|l| {
                    l.alert_id.as_deref() == Some(alert_id)
                        || (l.source_id.as_deref() == Some(source_id) && l.source_record_id.as_deref() == Some(source_record_id))
                })
            })
            .collect()
    }
}

// ------------------------------------------------------------ jurisdiction

/// How the country of an alert was determined. Only an issuing authority's own
/// national jurisdiction is used; global catalogues (e.g. USGS) never imply a
/// country, and nothing is guessed from coordinates or language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Jurisdiction {
    pub country_code: Option<String>,
    pub basis: &'static str,
}

pub fn jurisdiction_of(authority_role: Option<&str>, level: Option<&str>, id: Option<&str>) -> Jurisdiction {
    let code = id.filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic())).map(str::to_ascii_uppercase);
    match (authority_role, level, code) {
        (Some("warning_issuer"), Some("national"), Some(c)) => Jurisdiction { country_code: Some(c), basis: "issuing_authority_national" },
        (Some("warning_issuer"), _, _) => Jurisdiction { country_code: None, basis: "issuing_authority_subnational_or_unknown" },
        _ => Jurisdiction { country_code: None, basis: "not_determined" },
    }
}

// ------------------------------------------------------------------ bundle

#[derive(Clone, Debug)]
pub struct Reference {
    pub contacts: ContactsFile,
    pub guides: GuidesFile,
    pub facilities: FacilitiesFile,
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T, ReferenceError> {
    let s = std::fs::read_to_string(p).map_err(|e| ReferenceError::Read(p.display().to_string(), e))?;
    serde_json::from_str(&s).map_err(|e| ReferenceError::Json(p.display().to_string(), e))
}

impl Reference {
    pub fn load(contacts: &Path, guides: &Path, facilities: &Path) -> Result<Self, ReferenceError> {
        Self::from_parts(read_json(contacts)?, read_json(guides)?, read_json(facilities)?)
    }

    pub fn from_parts(contacts: ContactsFile, guides: GuidesFile, facilities: FacilitiesFile) -> Result<Self, ReferenceError> {
        contacts.validate()?;
        guides.validate()?;
        facilities.validate()?;
        Ok(Self { contacts, guides, facilities })
    }

    /// Counts by coverage level, for the meta endpoint and the final report.
    pub fn coverage_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for c in &self.contacts.countries {
            let k = match c.coverage {
                Coverage::Verified => "verified",
                Coverage::PartiallyVerified => "partially_verified",
                Coverage::Unverified => "unverified",
                Coverage::Unavailable => "unavailable",
            };
            *m.entry(k).or_insert(0) += 1;
        }
        m
    }
}

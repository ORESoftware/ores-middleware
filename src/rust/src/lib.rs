//! Routing-neutral `ores.docs-serving/v1` decision core.
//!
//! The crate does not register routes or load documentation bodies. Framework
//! adapters extract normalized request fields, call [`decide`], and either pass
//! through or ask an injected `api-docs` provider for the selected artifact.

use std::cmp::Ordering;
use std::collections::BTreeMap;

pub const DOCS_FORMAT_HEADER: &str = "X-Ores-Docs-Format";
pub const CONTRACT_DIGEST_HEADER: &str = "X-Ores-Contract-SHA256";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Representation {
    Html,
    Catalog,
    OpenApi,
    OpenRpc,
    Connect,
    HyperSchema,
}

impl Representation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Catalog => "catalog",
            Self::OpenApi => "openapi",
            Self::OpenRpc => "openrpc",
            Self::Connect => "connect",
            Self::HyperSchema => "hyper-schema",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "html" => Some(Self::Html),
            "catalog" => Some(Self::Catalog),
            "openapi" => Some(Self::OpenApi),
            "openrpc" => Some(Self::OpenRpc),
            "connect" => Some(Self::Connect),
            "hyper-schema" => Some(Self::HyperSchema),
            _ => None,
        }
    }

    const fn content_type(self) -> &'static str {
        match self {
            Self::Html => "text/html; charset=utf-8",
            Self::Catalog => "application/vnd.ores.api-docs+json; charset=utf-8",
            Self::OpenApi => "application/vnd.oai.openapi+json; charset=utf-8",
            Self::OpenRpc => "application/openrpc+json; charset=utf-8",
            Self::Connect => "application/vnd.ores.connect+json; charset=utf-8",
            Self::HyperSchema => "application/schema+json; charset=utf-8",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Pass,
    Serve,
    MethodNotAllowed,
    NotAcceptable,
    StoppedForEvaluation,
}

impl Action {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Serve => "serve",
            Self::MethodNotAllowed => "method-not-allowed",
            Self::NotAcceptable => "not-acceptable",
            Self::StoppedForEvaluation => "stopped-for-evaluation",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DocsRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub accept: Option<&'a str>,
    pub format: Option<&'a str>,
    pub runtime_contract_digest: Option<&'a str>,
    pub docs_contract_digest: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocsDecision {
    pub action: Action,
    pub status: Option<u16>,
    pub representation: Option<Representation>,
    pub head_only: bool,
    pub headers: BTreeMap<String, String>,
}

impl DocsDecision {
    fn pass() -> Self {
        Self {
            action: Action::Pass,
            status: None,
            representation: None,
            head_only: false,
            headers: BTreeMap::new(),
        }
    }

    fn reject(action: Action, status: u16, allow: bool) -> Self {
        let mut headers = base_headers("application/json; charset=utf-8");
        if allow {
            headers.insert("Allow".into(), "GET, HEAD".into());
        }
        Self {
            action,
            status: Some(status),
            representation: None,
            head_only: false,
            headers,
        }
    }
}

#[derive(Debug)]
struct MediaRange {
    media: String,
    quality: f32,
    index: usize,
}

fn base_headers(content_type: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("Cache-Control".into(), "no-store".into()),
        ("Pragma".into(), "no-cache".into()),
        ("X-Content-Type-Options".into(), "nosniff".into()),
        ("Referrer-Policy".into(), "no-referrer".into()),
        (
            "Permissions-Policy".into(),
            "camera=(), microphone=(), geolocation=()".into(),
        ),
        (
            "Vary".into(),
            format!("Accept, {DOCS_FORMAT_HEADER}"),
        ),
        ("Content-Type".into(), content_type.into()),
    ])
}

fn representation_headers(
    representation: Representation,
    docs_digest: Option<&str>,
) -> BTreeMap<String, String> {
    let mut headers = base_headers(representation.content_type());
    if representation == Representation::Html {
        headers.insert("X-Frame-Options".into(), "DENY".into());
        headers.insert(
            "Content-Security-Policy".into(),
            "default-src 'none'; style-src 'unsafe-inline'; img-src 'none'; frame-ancestors 'none'; base-uri 'none'; object-src 'none'; form-action 'none'; connect-src 'none'; script-src 'none'".into(),
        );
    }
    if let Some(digest) = docs_digest.filter(|value| !value.trim().is_empty()) {
        headers.insert(CONTRACT_DIGEST_HEADER.into(), digest.trim().into());
    }
    headers
}

fn fixed_representation(path: &str) -> Option<Representation> {
    match path {
        "/api/docs.json" | "/api-docs.json" => Some(Representation::Catalog),
        "/openapi.json" => Some(Representation::OpenApi),
        "/openrpc.json" => Some(Representation::OpenRpc),
        "/connect.json" => Some(Representation::Connect),
        "/hyper-schema.json" => Some(Representation::HyperSchema),
        _ => None,
    }
}

fn is_generic_path(path: &str) -> bool {
    matches!(path, "/docs/api" | "/api/docs" | "/api-docs")
}

fn parse_accept(value: Option<&str>) -> Vec<MediaRange> {
    let Some(value) = value.filter(|item| !item.trim().is_empty()) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    for (index, raw_part) in value.split(',').enumerate() {
        let mut pieces = raw_part.split(';');
        let media = pieces.next().unwrap_or_default().trim().to_ascii_lowercase();
        if media.is_empty() {
            continue;
        }
        let mut quality = 1.0_f32;
        let mut valid = true;
        for raw_parameter in pieces {
            let mut pair = raw_parameter.splitn(2, '=');
            let name = pair.next().unwrap_or_default().trim();
            if !name.eq_ignore_ascii_case("q") {
                continue;
            }
            let Some(raw_value) = pair.next() else {
                valid = false;
                break;
            };
            let Ok(parsed) = raw_value.trim().parse::<f32>() else {
                valid = false;
                break;
            };
            if !(0.0..=1.0).contains(&parsed) {
                valid = false;
                break;
            }
            quality = parsed;
        }
        if valid && quality > 0.0 {
            ranges.push(MediaRange {
                media,
                quality,
                index,
            });
        }
    }
    ranges.sort_by(|left, right| {
        right
            .quality
            .partial_cmp(&left.quality)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.index.cmp(&right.index))
    });
    ranges
}

fn media_representation(media: &str) -> Option<Representation> {
    match media {
        "*/*" => Some(Representation::Html),
        "application/*" => Some(Representation::Catalog),
        "text/html" => Some(Representation::Html),
        "application/vnd.ores.api-docs+json" | "application/json" => {
            Some(Representation::Catalog)
        }
        "application/vnd.oai.openapi+json" | "application/openapi+json" => {
            Some(Representation::OpenApi)
        }
        "application/openrpc+json" => Some(Representation::OpenRpc),
        "application/vnd.ores.connect+json" => Some(Representation::Connect),
        "application/schema+json" => Some(Representation::HyperSchema),
        _ => None,
    }
}

fn negotiate_generic(accept: Option<&str>) -> Option<Representation> {
    let ranges = parse_accept(accept);
    if ranges.is_empty() {
        return if accept.is_none_or(|value| value.trim().is_empty()) {
            Some(Representation::Html)
        } else {
            None
        };
    }
    ranges
        .into_iter()
        .find_map(|range| media_representation(&range.media))
}

fn accepts_representation(accept: Option<&str>, representation: Representation) -> bool {
    let Some(value) = accept.filter(|item| !item.trim().is_empty()) else {
        return true;
    };
    let ranges = parse_accept(Some(value));
    if ranges.is_empty() {
        return false;
    }
    ranges.into_iter().any(|range| {
        range.media == "*/*"
            || (representation != Representation::Html
                && matches!(range.media.as_str(), "application/*" | "application/json"))
            || media_representation(&range.media) == Some(representation)
    })
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest_failure(runtime_digest: Option<&str>, docs_digest: Option<&str>) -> bool {
    let runtime = runtime_digest
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let docs = docs_digest
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if runtime.is_some_and(|value| !valid_digest(value))
        || docs.is_some_and(|value| !valid_digest(value))
    {
        return true;
    }
    runtime.is_some_and(|expected| docs != Some(expected))
}

/// Decide whether a normalized request should pass through or receive an
/// already-generated API-document artifact.
#[must_use]
pub fn decide(request: DocsRequest<'_>) -> DocsDecision {
    let path = request.path.split('?').next().unwrap_or_default();
    let generic = is_generic_path(path);
    let fixed = fixed_representation(path);
    if !generic && fixed.is_none() {
        return DocsDecision::pass();
    }

    let method = request.method.to_ascii_uppercase();
    if !matches!(method.as_str(), "GET" | "HEAD") {
        return DocsDecision::reject(Action::MethodNotAllowed, 405, true);
    }

    if digest_failure(request.runtime_contract_digest, request.docs_contract_digest) {
        return DocsDecision::reject(Action::StoppedForEvaluation, 503, false);
    }

    let format = match request.format.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => match Representation::parse(value) {
            Some(value) => Some(value),
            None => return DocsDecision::reject(Action::NotAcceptable, 406, false),
        },
        None => None,
    };

    let representation = if generic {
        match format.or_else(|| negotiate_generic(request.accept)) {
            Some(value) => value,
            None => return DocsDecision::reject(Action::NotAcceptable, 406, false),
        }
    } else {
        let value = fixed.expect("recognized fixed docs path");
        if format.is_some_and(|selected| selected != value) {
            return DocsDecision::reject(Action::NotAcceptable, 406, false);
        }
        value
    };

    if !accepts_representation(request.accept, representation) {
        return DocsDecision::reject(Action::NotAcceptable, 406, false);
    }

    DocsDecision {
        action: Action::Serve,
        status: Some(200),
        representation: Some(representation),
        head_only: method == "HEAD",
        headers: representation_headers(representation, request.docs_contract_digest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn optional(value: &str) -> Option<&str> {
        (value != "-").then_some(value)
    }

    #[test]
    fn shared_conformance_fixture() {
        let fixture = include_str!("../../../fixtures/docs-serving-conformance.tsv");
        for line in fixture.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 11, "invalid fixture row: {line}");
            let name = fields[0];
            let decision = decide(DocsRequest {
                method: fields[1],
                path: fields[2],
                accept: optional(fields[3]),
                format: optional(fields[4]),
                runtime_contract_digest: optional(fields[5]),
                docs_contract_digest: optional(fields[6]),
            });
            assert_eq!(decision.action.as_str(), fields[7], "{name}: action");
            let expected_status = optional(fields[8]).map(|value| value.parse::<u16>().unwrap());
            assert_eq!(decision.status, expected_status, "{name}: status");
            let expected_representation = optional(fields[9]);
            assert_eq!(
                decision.representation.map(Representation::as_str),
                expected_representation,
                "{name}: representation"
            );
            assert_eq!(decision.head_only, fields[10] == "true", "{name}: headOnly");

            if decision.action == Action::Pass {
                assert!(decision.headers.is_empty(), "{name}: pass headers");
            } else {
                assert_eq!(
                    decision.headers.get("Cache-Control").map(String::as_str),
                    Some("no-store"),
                    "{name}: no-store"
                );
                assert!(
                    decision
                        .headers
                        .get("Vary")
                        .is_some_and(|value| value.contains(DOCS_FORMAT_HEADER)),
                    "{name}: vary"
                );
            }
            if decision.action == Action::MethodNotAllowed {
                assert_eq!(
                    decision.headers.get("Allow").map(String::as_str),
                    Some("GET, HEAD"),
                    "{name}: allow"
                );
            }
            if decision.representation == Some(Representation::Html) {
                assert_eq!(
                    decision.headers.get("X-Frame-Options").map(String::as_str),
                    Some("DENY"),
                    "{name}: frame"
                );
                assert!(
                    decision
                        .headers
                        .get("Content-Security-Policy")
                        .is_some_and(|value| value.contains("frame-ancestors 'none'")),
                    "{name}: csp"
                );
            }
            if optional(fields[6]).is_some_and(|digest| digest.len() == 64)
                && decision.action == Action::Serve
            {
                assert_eq!(
                    decision
                        .headers
                        .get(CONTRACT_DIGEST_HEADER)
                        .map(String::as_str),
                    optional(fields[6]),
                    "{name}: digest"
                );
            }
        }
    }
}

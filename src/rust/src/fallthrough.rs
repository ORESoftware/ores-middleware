use http::{
    HeaderMap, HeaderValue, Method, StatusCode,
    header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS},
};
use serde::Serialize;

pub const UNMATCHED_ROUTE_ERROR_CODE: &str = "ores.route.unmatched";
pub const UNMATCHED_ROUTE_PROBLEM_TYPE: &str = "urn:ores:error:route-unmatched";
pub const UNMATCHED_ROUTE_TITLE: &str = "No route matched";
pub const UNMATCHED_ROUTE_DETAIL: &str = "The request target is not handled by this server.";

/// Selects the HTTP status used at the final server/router boundary.
///
/// `NotFound` is the standards-correct default when the request reached the
/// intended origin but no application route claims its target resource.
/// `MisdirectedAuthority` is only for an origin/connection mismatch where HTTP
/// 421 is actually appropriate. A known route with an unsupported method
/// remains a router-owned 405 response with `Allow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FallthroughStatusMode {
    #[default]
    NotFound,
    MisdirectedAuthority,
}

impl FallthroughStatusMode {
    pub const fn status_code(self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MisdirectedAuthority => StatusCode::MISDIRECTED_REQUEST,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FallthroughConfig {
    pub status_mode: FallthroughStatusMode,
}

impl FallthroughConfig {
    /// Select HTTP 421 only when the request was directed at an origin or
    /// connection context for which this server is not authoritative.
    pub const fn misdirected_authority() -> Self {
        Self {
            status_mode: FallthroughStatusMode::MisdirectedAuthority,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallthroughResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

#[derive(Serialize)]
struct ProblemDetails<'a> {
    #[serde(rename = "type")]
    problem_type: &'a str,
    title: &'a str,
    status: u16,
    code: &'a str,
    detail: &'a str,
}

/// Builds the canonical ORES final/fall-through response.
///
/// No request path, query, method, route table, or framework detail is echoed.
/// HEAD receives the same metadata and would-be Content-Length as GET, but no
/// body bytes. The stable `ores.route.unmatched` code distinguishes this
/// router fall-through from resource-level 404 responses in telemetry.
pub fn final_fallthrough_response(
    method: &Method,
    config: FallthroughConfig,
) -> FallthroughResponse {
    let status = config.status_mode.status_code();
    let encoded = serde_json::to_vec(&ProblemDetails {
        problem_type: UNMATCHED_ROUTE_PROBLEM_TYPE,
        title: UNMATCHED_ROUTE_TITLE,
        status: status.as_u16(),
        code: UNMATCHED_ROUTE_ERROR_CODE,
        detail: UNMATCHED_ROUTE_DETAIL,
    })
    .expect("static fallthrough problem details must serialize");

    let content_length = HeaderValue::from_str(&encoded.len().to_string())
        .expect("serialized problem length is a valid header value");
    let headers = [
        (
            CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json; charset=utf-8"),
        ),
        (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        (CONTENT_LENGTH, content_length),
    ]
    .into_iter()
    .collect();
    let body = if *method == Method::HEAD {
        Vec::new()
    } else {
        encoded
    };

    FallthroughResponse {
        status,
        headers,
        body,
    }
}

#[cfg(feature = "axum")]
impl axum::response::IntoResponse for FallthroughResponse {
    fn into_response(self) -> axum::response::Response {
        let mut response = axum::response::Response::new(axum::body::Body::from(self.body));
        *response.status_mut() = self.status;
        *response.headers_mut() = self.headers;
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_not_found_with_stable_fallthrough_code() {
        let response = final_fallthrough_response(&Method::GET, FallthroughConfig::default());
        assert_eq!(response.status, StatusCode::NOT_FOUND);
        assert_eq!(
            response.headers.get(CACHE_CONTROL).and_then(|v| v.to_str().ok()),
            Some("no-store")
        );
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["code"], UNMATCHED_ROUTE_ERROR_CODE);
        assert_eq!(body["status"], 404);
    }

    #[test]
    fn supports_explicit_misdirected_authority_mode() {
        let response = final_fallthrough_response(
            &Method::GET,
            FallthroughConfig::misdirected_authority(),
        );
        assert_eq!(response.status, StatusCode::MISDIRECTED_REQUEST);
        let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(body["status"], 421);
        assert_eq!(body["code"], UNMATCHED_ROUTE_ERROR_CODE);
    }

    #[test]
    fn head_omits_body_but_preserves_would_be_length() {
        let get = final_fallthrough_response(&Method::GET, FallthroughConfig::default());
        let head = final_fallthrough_response(&Method::HEAD, FallthroughConfig::default());
        assert!(head.body.is_empty());
        assert_eq!(head.status, get.status);
        assert_eq!(
            head.headers.get(CONTENT_LENGTH),
            get.headers.get(CONTENT_LENGTH)
        );
        let advertised_length = get
            .headers
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap();
        assert_eq!(advertised_length, get.body.len());
    }

    #[test]
    fn body_does_not_echo_request_target_or_method() {
        let response = final_fallthrough_response(&Method::DELETE, FallthroughConfig::default());
        let body = String::from_utf8(response.body).unwrap();
        assert!(!body.contains("DELETE"));
        assert!(!body.contains("/"));
    }
}

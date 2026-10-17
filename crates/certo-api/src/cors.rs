//! CORS handling equivalent to the Go server's `rs/cors` setup: preflights are answered
//! directly with 204, actual requests get `Access-Control-Allow-Origin` when allowed.
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

const ALLOWED_METHODS: [&str; 5] = ["GET", "POST", "PUT", "DELETE", "OPTIONS"];
const ALLOWED_HEADERS: [&str; 3] = ["Authorization", "Content-Type", "Origin"];

/// Allowed origins: everything, or exact and single-`*` wildcard patterns.
#[derive(Debug, Clone)]
pub(crate) struct Cors {
    all: bool,
    exact: Vec<String>,
    wildcard: Vec<(String, String)>,
}

impl Cors {
    pub(crate) fn new(origins: &[String]) -> Self {
        let mut cors = Self {
            all: origins.is_empty(),
            exact: Vec::new(),
            wildcard: Vec::new(),
        };
        for origin in origins.iter().map(|o| o.to_lowercase()) {
            if origin == "*" {
                return Self {
                    all: true,
                    exact: Vec::new(),
                    wildcard: Vec::new(),
                };
            }
            match origin.split_once('*') {
                Some((pre, suf)) => cors.wildcard.push((pre.to_owned(), suf.to_owned())),
                None => cors.exact.push(origin),
            }
        }
        cors
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        if self.all {
            return true;
        }
        let origin = origin.to_lowercase();
        self.exact.contains(&origin)
            || self.wildcard.iter().any(|(pre, suf)| {
                origin.len() >= pre.len() + suf.len()
                    && origin.starts_with(pre.as_str())
                    && origin.ends_with(suf.as_str())
            })
    }

    fn allow_origin(&self, origin: &str) -> Option<HeaderValue> {
        if self.all {
            Some(HeaderValue::from_static("*"))
        } else {
            HeaderValue::from_str(origin).ok()
        }
    }
}

fn method_allowed(method: &str) -> bool {
    let method = method.to_uppercase();
    ALLOWED_METHODS.contains(&method.as_str())
}

/// `content-type` → `Content-Type`.
fn canonical_header(name: &str) -> String {
    let mut upper = true;
    name.chars()
        .map(|c| {
            let out = if upper {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            };
            upper = c == '-';
            out
        })
        .collect()
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
}

fn preflight(cors: &Cors, req: &HeaderMap) -> Response {
    let mut resp = StatusCode::NO_CONTENT.into_response();
    let headers = resp.headers_mut();
    for v in [
        "Origin",
        "Access-Control-Request-Method",
        "Access-Control-Request-Headers",
    ] {
        headers.append(header::VARY, HeaderValue::from_static(v));
    }
    let origin = header_str(req, "origin");
    let method = header_str(req, "access-control-request-method");
    let requested: Vec<String> = header_str(req, "access-control-request-headers")
        .split(',')
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(canonical_header)
        .collect();
    if origin.is_empty()
        || !cors.origin_allowed(origin)
        || !method_allowed(method)
        || !requested
            .iter()
            .all(|h| ALLOWED_HEADERS.contains(&h.as_str()))
    {
        return resp;
    }
    let Some(allow_origin) = cors.allow_origin(origin) else {
        return resp;
    };
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, allow_origin);
    if let Ok(v) = HeaderValue::from_str(&method.to_uppercase()) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, v);
    }
    if !requested.is_empty()
        && let Ok(v) = HeaderValue::from_str(&requested.join(", "))
    {
        headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, v);
    }
    resp
}

pub(crate) async fn layer(State(cors): State<Cors>, req: Request, next: Next) -> Response {
    if req.method() == Method::OPTIONS
        && req.headers().contains_key("access-control-request-method")
    {
        return preflight(&cors, req.headers());
    }
    let origin = header_str(req.headers(), "origin").to_owned();
    let allowed =
        !origin.is_empty() && cors.origin_allowed(&origin) && method_allowed(req.method().as_str());
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    headers.append(header::VARY, HeaderValue::from_static("Origin"));
    if allowed && let Some(v) = cors.allow_origin(&origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_matching() {
        assert!(Cors::new(&[]).origin_allowed("https://x.test"));
        let cors = Cors::new(&["https://A.test".into(), "https://*.b.test".into()]);
        assert!(cors.origin_allowed("https://a.test"));
        assert!(cors.origin_allowed("https://x.b.test"));
        assert!(!cors.origin_allowed("https://b.test"));
        assert!(!cors.origin_allowed("https://c.test"));
        assert!(Cors::new(&["x".into(), "*".into()]).origin_allowed("y"));
    }

    #[test]
    fn header_canonicalization() {
        assert_eq!(canonical_header("content-TYPE"), "Content-Type");
        assert_eq!(canonical_header("authorization"), "Authorization");
    }
}

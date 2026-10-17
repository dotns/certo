//! Dashboard SPA: a `web/dist` directory in the working directory overrides the files
//! embedded at build time; unknown paths fall back to `index.html` for client routing.
use std::{borrow::Cow, path::PathBuf};

use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Dist;

const INDEX: &str = "index.html";

/// Where dashboard files are served from.
#[derive(Debug, Clone)]
pub(crate) enum Spa {
    Dir(PathBuf),
    Embedded,
}

impl Spa {
    /// Picks the filesystem override, then the embedded build; `None` if neither has files.
    pub(crate) fn detect() -> Option<Self> {
        let dir = PathBuf::from("web/dist");
        if dir.is_dir() {
            tracing::info!(dir = %dir.display(), "serving SPA from filesystem");
            return Some(Self::Dir(dir));
        }
        if Dist::get(INDEX).is_some() {
            tracing::info!("serving SPA from embedded files");
            return Some(Self::Embedded);
        }
        None
    }

    async fn load(&self, name: &str) -> Option<Cow<'static, [u8]>> {
        match self {
            Self::Embedded => Dist::get(name).map(|f| f.data),
            Self::Dir(dir) => {
                // Only plain relative segments; no traversal out of the directory.
                if name
                    .split('/')
                    .any(|s| s.is_empty() || s == "." || s == ".." || s.contains('\\'))
                {
                    return None;
                }
                let path = dir.join(name);
                if !tokio::fs::metadata(&path).await.ok()?.is_file() {
                    return None;
                }
                tokio::fs::read(path).await.ok().map(Cow::Owned)
            }
        }
    }

    /// Serves the file at `path`, or `index.html` when it does not exist.
    pub(crate) async fn serve(&self, path: &str) -> Response {
        let name = match path.trim_start_matches('/') {
            "" => INDEX,
            n => n,
        };
        let (name, data) = match self.load(name).await {
            Some(data) => (name, data),
            None => match self.load(INDEX).await {
                Some(data) => (INDEX, data),
                None => return StatusCode::NOT_FOUND.into_response(),
            },
        };
        let mime = mime_guess::from_path(name).first_or_octet_stream();
        let textual = mime.type_() == mime_guess::mime::TEXT
            || mime.subtype() == mime_guess::mime::JAVASCRIPT;
        let mime = if textual && mime.get_param("charset").is_none() {
            format!("{mime}; charset=utf-8")
        } else {
            mime.to_string()
        };
        let mut resp = data.into_owned().into_response();
        if let Ok(v) = HeaderValue::from_str(&mime) {
            resp.headers_mut().insert(header::CONTENT_TYPE, v);
        }
        resp
    }
}

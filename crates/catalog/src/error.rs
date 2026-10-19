use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("network error: {0}")]
    Http(reqwest::Error),
    /// The connection was refused or could not be made. For a call to the
    /// local catalog engine this means it has stopped (or was never started):
    /// retrying cannot help until it is restarted.
    #[error("could not connect ({0})")]
    Unreachable(String),
    /// A page or cover was larger than the allowed size.
    #[error("the response was larger than the {limit} byte limit")]
    TooLarge { limit: u64 },
    /// A download received no data for too long.
    #[error("the download stalled: {0}")]
    Stalled(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("checksum mismatch for {asset}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        asset: String,
        expected: String,
        actual: String,
    },
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
    #[error("could not unpack the download: {0}")]
    Archive(String),
    #[error("the Suwayomi install is incomplete: {0}")]
    BadInstall(String),
    #[error("the server did not become ready within {0:?}")]
    StartTimeout(Duration),
    #[error("the server stopped during start-up ({0}); see the log at {1}")]
    ServerExited(String, String),

    /// The source needs the built-in browser (Chromium), which is turned off.
    #[error("this source needs browser support, which is turned off")]
    NeedsBrowser,
    /// The source is behind a Cloudflare challenge that needs FlareSolverr.
    #[error("this source is protected by Cloudflare and needs a FlareSolverr service")]
    NeedsFlareSolverr,
    /// The source's website answered with an HTTP error.
    #[error("the source's website returned HTTP {0}")]
    SourceHttp(u16),

    #[error("server error: {0}")]
    Graphql(String),
    #[error("unexpected response from the server: {0}")]
    BadResponse(String),
}

impl From<reqwest::Error> for CatalogError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_connect() {
            CatalogError::Unreachable(e.to_string())
        } else {
            CatalogError::Http(e)
        }
    }
}

pub type Result<T> = std::result::Result<T, CatalogError>;

/// Turn a GraphQL error message from Suwayomi into a specific error the UI
/// can act on. The messages are the ones observed from the real server.
pub fn classify_graphql_error(message: &str) -> CatalogError {
    if message.contains("WebView") || message.contains("CEF is disabled") {
        return CatalogError::NeedsBrowser;
    }
    if message.contains("Cloudflare bypass") {
        return CatalogError::NeedsFlareSolverr;
    }
    if let Some(rest) = message.split("HTTP error ").nth(1) {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(code) = digits.parse() {
            return CatalogError::SourceHttp(code);
        }
    }
    CatalogError::Graphql(message.lines().next().unwrap_or(message).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_observed_server_messages() {
        assert!(matches!(
            classify_graphql_error(
                "Exception while fetching data (/fetchChapterPages) : Open chapter in WebView, then try again (token not found)."
            ),
            CatalogError::NeedsBrowser
        ));
        // Seen from Comikey when the embedded browser is switched off.
        assert!(matches!(
            classify_graphql_error(
                "Exception while fetching data (/fetchChapterPages) : CEF is disabled"
            ),
            CatalogError::NeedsBrowser
        ));
        assert!(matches!(
            classify_graphql_error(
                "Exception while fetching data (/fetchSourceManga) : Cloudflare bypass currently disabled\n\njava.io.IOException"
            ),
            CatalogError::NeedsFlareSolverr
        ));
        assert!(matches!(
            classify_graphql_error("eu.kanade.tachiyomi.network.HttpException: HTTP error 404"),
            CatalogError::SourceHttp(404)
        ));
        match classify_graphql_error("something odd\nstack trace line") {
            CatalogError::Graphql(m) => assert_eq!(m, "something odd"),
            other => panic!("unexpected {other:?}"),
        }
    }
}

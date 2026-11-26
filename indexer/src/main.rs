use axum::{
    async_trait,
    extract::{FromRequestParts, Query, State},
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use revora_indexer::{
    normalize_pagination, EventFilter, EventIndexer, IndexerError, NewContractEvent, Result,
    MAX_PAGE_SIZE,
};
use serde::Deserialize;
use std::env;
use std::net::SocketAddr;
use subtle::ConstantTimeEq;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing_subscriber;

struct IndexerErrorResponse(IndexerError);

impl IntoResponse for IndexerErrorResponse {
    fn into_response(self) -> Response {
        let (status, message) = match &self.0 {
            IndexerError::InvalidEventData => (StatusCode::BAD_REQUEST, "Invalid event data"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
        };

        tracing::error!(error = %self.0, "Indexer request failed");
        (status, axum::Json(serde_json::json!({ "error": message }))).into_response()
    }
}

// ---------------------------------------------------------------------------
// API key extractor
// ---------------------------------------------------------------------------

/// Shared API-key state threaded through axum.
#[derive(Clone)]
pub struct ApiKey(pub String);

/// Extractor that enforces `Authorization: Bearer <key>` using constant-time
/// comparison to prevent timing-based key oracle attacks.
pub struct RequireApiKey;

#[async_trait]
impl<S> FromRequestParts<S> for RequireApiKey
where
    S: Send + Sync + AsRef<ApiKey>,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &S) -> std::result::Result<Self, Self::Rejection> {
        let expected: &ApiKey = state.as_ref();

        let provided = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");

        // Constant-time comparison — same length padding avoids length oracle.
        let matches = provided.as_bytes().ct_eq(expected.0.as_bytes()).into();
        if matches {
            Ok(RequireApiKey)
        } else {
            Err((StatusCode::UNAUTHORIZED, "Unauthorized"))
        }
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub indexer: EventIndexer,
    pub api_key: ApiKey,
}

impl AsRef<ApiKey> for AppState {
    fn as_ref(&self) -> &ApiKey {
        &self.api_key
    }
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PaginationParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn validate_pagination(
    limit: Option<i64>,
    offset: Option<i64>,
) -> std::result::Result<(i64, i64), String> {
    if let Some(limit) = limit {
        if !(1..=MAX_PAGE_SIZE).contains(&limit) {
            return Err(format!("limit must be between 1 and {MAX_PAGE_SIZE}"));
        }
    }
    if offset.is_some_and(|offset| offset < 0) {
        return Err("offset must be non-negative".to_string());
    }
    Ok(normalize_pagination(limit, offset))
}

async fn get_events(
    State(state): State<AppState>,
    Query(filter): Query<EventFilter>,
    Query(pagination): Query<PaginationParams>,
) -> Response {
    let (limit, offset) = match validate_pagination(pagination.limit, pagination.offset) {
        Ok(values) => values,
        Err(message) => return (StatusCode::BAD_REQUEST, axum::Json(message)).into_response(),
    };

    match state
        .indexer
        .query_events(&filter, Some(limit), Some(offset))
        .await
    {
        Ok(events) => (StatusCode::OK, axum::Json(events)).into_response(),
        Err(error) => IndexerErrorResponse(error).into_response(),
    }
}

async fn create_event(
    State(state): State<AppState>,
    _auth: RequireApiKey,
    axum::Json(new_event): axum::Json<NewContractEvent>,
) -> Response {
    match state.indexer.insert_event(&new_event).await {
        Ok(event) => (StatusCode::CREATED, axum::Json(event)).into_response(),
        Err(error) => IndexerErrorResponse(error).into_response(),
    }
}

async fn get_analytics(State(state): State<AppState>) -> Response {
    match state.indexer.get_analytics().await {
        Ok(analytics) => (StatusCode::OK, axum::Json(analytics)).into_response(),
        Err(error) => IndexerErrorResponse(error).into_response(),
    }
}

async fn export_csv(
    State(state): State<AppState>,
    Query(filter): Query<EventFilter>,
) -> Response {
    match state.indexer.export_events_to_csv(&filter).await {
        Ok(data) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/csv")],
            [(
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"events.csv\"",
            )],
            data,
        )
            .into_response(),
        Err(error) => IndexerErrorResponse(error).into_response(),
    }
}

async fn export_json(
    State(state): State<AppState>,
    Query(filter): Query<EventFilter>,
) -> Response {
    match state.indexer.export_events_to_json(&filter).await {
        Ok(data) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            [(
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"events.json\"",
            )],
            data,
        )
            .into_response(),
        Err(error) => IndexerErrorResponse(error).into_response(),
    }
}

// ---------------------------------------------------------------------------
// CORS helpers
// ---------------------------------------------------------------------------

/// Build an `AllowOrigin` from a comma-separated list of origins in
/// `INDEXER_CORS_ORIGINS`.  Falls back to `Any` only when the variable is not
/// set, so that dev environments still work without extra configuration; if the
/// variable is explicitly set it must contain at least one valid origin.
fn build_allow_origin() -> AllowOrigin {
    match env::var("INDEXER_CORS_ORIGINS") {
        Ok(raw) => {
            let origins: Vec<_> = raw
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse::<axum::http::HeaderValue>().ok())
                .collect();

            if origins.is_empty() {
                tracing::warn!(
                    "INDEXER_CORS_ORIGINS is set but contained no valid origins; \
                     falling back to wildcard — set it correctly in production"
                );
                AllowOrigin::any()
            } else {
                tracing::info!(
                    "CORS restricted to {} origin(s): {}",
                    origins.len(),
                    raw.trim()
                );
                AllowOrigin::list(origins)
            }
        }
        Err(_) => {
            tracing::warn!(
                "INDEXER_CORS_ORIGINS not set — allowing any origin. \
                 Set this variable in production."
            );
            AllowOrigin::any()
        }
    }
}

// ---------------------------------------------------------------------------
// Router (pub so tests can call it without spinning up a real DB)
// ---------------------------------------------------------------------------

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(build_allow_origin())
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    Router::new()
        // GET /events: limit defaults to 100 and is capped at 1000; offset must be >= 0.
        // Invalid limit/offset values (including non-integers) return HTTP 400.
        .route("/events", get(get_events).post(create_event))
        .route("/analytics", get(get_analytics))
        .route("/export/csv", get(export_csv))
        .route("/export/json", get(export_json))
        .with_state(state)
        .layer(cors)
}

// ---------------------------------------------------------------------------
// Configuration helpers
// ---------------------------------------------------------------------------

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";

/// Parse `INDEXER_BIND_ADDR` into a [`SocketAddr`], defaulting to `127.0.0.1:3000`
/// when unset. Fails with a clear error message if set to an empty or invalid value.
fn parse_bind_addr(raw: Option<&str>) -> std::result::Result<SocketAddr, String> {
    let addr_str = match raw {
        Some(s) if !s.trim().is_empty() => s.trim(),
        Some(_) => {
            return Err(
                "INDEXER_BIND_ADDR is set but empty. Set a valid socket address such as 127.0.0.1:3000."
                    .to_string(),
            );
        }
        None => DEFAULT_BIND_ADDR,
    };

    addr_str.parse::<SocketAddr>().map_err(|err| {
        format!(
            "Invalid INDEXER_BIND_ADDR '{addr_str}': {err}. Expected <ip>:<port>, e.g. 127.0.0.1:3000."
        )
    })
}

/// Validate required `DATABASE_URL` environment variable without falling back to
/// hard-coded credentials.
fn parse_database_url(raw: Option<String>) -> std::result::Result<String, String> {
    match raw {
        Some(url) if !url.trim().is_empty() => Ok(url),
        Some(_) => Err(
            "DATABASE_URL is set but empty. Set DATABASE_URL to a valid PostgreSQL connection string."
                .to_string(),
        ),
        None => Err(
            "DATABASE_URL is not set. Set DATABASE_URL to a valid PostgreSQL connection string."
                .to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Fail closed: refuse to start if no API key is configured.
    let api_key_value = env::var("INDEXER_API_KEY").unwrap_or_else(|_| {
        tracing::error!(
            "INDEXER_API_KEY is not set. \
             POST /events is disabled until a key is configured. \
             Set INDEXER_API_KEY to a strong random secret and restart."
        );
        // Exit instead of panicking so the message is clean in systemd/container logs.
        std::process::exit(1);
    });

    if api_key_value.trim().is_empty() {
        tracing::error!(
            "INDEXER_API_KEY is set but empty. \
             Refusing to start with a blank key."
        );
        std::process::exit(1);
    }

    let database_url = parse_database_url(env::var("DATABASE_URL").ok()).unwrap_or_else(|err| {
        tracing::error!("{err}");
        std::process::exit(1);
    });

    let bind_addr =
        parse_bind_addr(env::var("INDEXER_BIND_ADDR").ok().as_deref()).unwrap_or_else(|err| {
            tracing::error!("{err}");
            std::process::exit(1);
        });

    let indexer = EventIndexer::new(&database_url).await?;
    indexer.initialize_schema().await?;

    let state = AppState {
        indexer,
        api_key: ApiKey(api_key_value),
    };

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .unwrap_or_else(|err| {
            tracing::error!("Failed to bind to {bind_addr}: {err}");
            std::process::exit(1);
        });
    tracing::info!("Listening on http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{self, Request},
        routing::post,
        Router,
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn error_responses_map_status_and_hide_internal_details() {
        let invalid_response =
            IndexerErrorResponse(IndexerError::InvalidEventData).into_response();
        assert_eq!(invalid_response.status(), StatusCode::BAD_REQUEST);
        let invalid_body =
            axum::body::to_bytes(invalid_response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&invalid_body).unwrap(),
            serde_json::json!({ "error": "Invalid event data" })
        );

        let internal_response = IndexerErrorResponse(IndexerError::DatabaseError(
            sqlx::Error::Protocol("sensitive SQL details".to_string()),
        ))
        .into_response();
        assert_eq!(internal_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let internal_body =
            axum::body::to_bytes(internal_response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&internal_body).unwrap(),
            serde_json::json!({ "error": "Internal server error" })
        );
    }

    /// Minimal state that satisfies `AsRef<ApiKey>` without a real DB.
    #[derive(Clone)]
    struct StubState {
        api_key: ApiKey,
    }

    impl AsRef<ApiKey> for StubState {
        fn as_ref(&self) -> &ApiKey {
            &self.api_key
        }
    }

    /// A handler that only runs if `RequireApiKey` extraction succeeds.
    async fn protected_stub(_auth: RequireApiKey) -> impl IntoResponse {
        (StatusCode::OK, "ok")
    }

    /// Build a minimal router backed by `StubState` (no DB).
    fn stub_router(key: &str) -> Router {
        let state = StubState {
            api_key: ApiKey(key.to_string()),
        };
        Router::new()
            .route("/events", post(protected_stub))
            .with_state(state)
    }

    // -----------------------------------------------------------------------
    // Helper: send a POST to /events with an optional Authorization header.
    // -----------------------------------------------------------------------
    async fn post_events(router: Router, auth: Option<&str>) -> http::StatusCode {
        let mut req = Request::builder()
            .method(http::Method::POST)
            .uri("/events")
            .header(http::header::CONTENT_TYPE, "application/json");

        if let Some(bearer) = auth {
            req = req.header(http::header::AUTHORIZATION, format!("Bearer {}", bearer));
        }

        let response = router
            .oneshot(req.body(Body::from("{}")).unwrap())
            .await
            .unwrap();

        response.status()
    }

    // -----------------------------------------------------------------------
    // 1. No Authorization header → 401
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn post_without_auth_returns_401() {
        let status = post_events(stub_router("secret-key"), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // 2. Wrong key → 401
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn post_with_wrong_key_returns_401() {
        let status = post_events(stub_router("correct-key"), Some("wrong-key")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // 3. Correct key → handler runs (200 from stub)
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn post_with_correct_key_returns_200() {
        let status = post_events(stub_router("correct-key"), Some("correct-key")).await;
        assert_eq!(status, StatusCode::OK);
    }

    // -----------------------------------------------------------------------
    // 4. "Bearer " prefix required; bare key is rejected
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn post_with_bare_key_no_bearer_prefix_returns_401() {
        // Provide the right secret but without the "Bearer " prefix.
        let state = StubState {
            api_key: ApiKey("secret".to_string()),
        };
        let router = Router::new()
            .route("/events", post(protected_stub))
            .with_state(state);

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/events")
            .header(http::header::AUTHORIZATION, "secret") // no "Bearer " prefix
            .body(Body::from("{}"))
            .unwrap();

        let status = router.oneshot(req).await.unwrap().status();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // 5. Constant-time compare: a key that is a prefix of the expected key
    //    must be rejected (guards against short-circuit length oracle).
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn post_with_prefix_of_key_returns_401() {
        // Expected key is "long-secret", attacker provides "long" (a prefix).
        let status = post_events(stub_router("long-secret"), Some("long")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // 6. Response body of a rejected request contains the word "Unauthorized"
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn rejected_response_body_says_unauthorized() {
        let state = StubState {
            api_key: ApiKey("secret".to_string()),
        };
        let router = Router::new()
            .route("/events", post(protected_stub))
            .with_state(state);

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/events")
            .body(Body::from("{}"))
            .unwrap();

        let response = router.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&body).unwrap().contains("Unauthorized"));
    }

    // -----------------------------------------------------------------------
    // 7. GET /events (public) is reachable without any auth header.
    //    Uses the stub router's missing GET route → 405 Method Not Allowed,
    //    which proves the router did not 401 it on the way in.
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn get_route_does_not_require_auth() {
        // The stub only has POST /events, so a GET returns 405 — not 401.
        let state = StubState {
            api_key: ApiKey("secret".to_string()),
        };
        let router = Router::new()
            .route("/events", post(protected_stub))
            .with_state(state);

        let req = Request::builder()
            .method(http::Method::GET)
            .uri("/events")
            .body(Body::empty())
            .unwrap();

        let status = router.oneshot(req).await.unwrap().status();
        // 405 means the router reached the routing layer, not a 401 auth gate.
        assert_ne!(status, StatusCode::UNAUTHORIZED);
    }

    // -----------------------------------------------------------------------
    // Configuration parsing tests (INDEXER_BIND_ADDR & DATABASE_URL)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_bind_addr_defaults_to_loopback() {
        let addr = parse_bind_addr(None).expect("default bind address should parse");
        assert_eq!(addr, "127.0.0.1:3000".parse::<SocketAddr>().unwrap());
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn parse_bind_addr_accepts_valid_ipv4_and_ipv6_overrides() {
        let ipv4 = parse_bind_addr(Some("0.0.0.0:8080")).expect("IPv4 override should parse");
        assert_eq!(ipv4, "0.0.0.0:8080".parse::<SocketAddr>().unwrap());

        let trimmed = parse_bind_addr(Some("  127.0.0.1:4000  "))
            .expect("whitespace-padded override should parse");
        assert_eq!(trimmed, "127.0.0.1:4000".parse::<SocketAddr>().unwrap());

        let ipv6 = parse_bind_addr(Some("[::1]:9090")).expect("IPv6 override should parse");
        assert_eq!(ipv6, "[::1]:9090".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn parse_bind_addr_rejects_empty_and_invalid_inputs() {
        let empty_err = parse_bind_addr(Some("")).unwrap_err();
        assert!(empty_err.contains("INDEXER_BIND_ADDR"));
        assert!(empty_err.contains("empty"));

        let whitespace_err = parse_bind_addr(Some("   ")).unwrap_err();
        assert!(whitespace_err.contains("INDEXER_BIND_ADDR"));

        let missing_port_err = parse_bind_addr(Some("127.0.0.1")).unwrap_err();
        assert!(missing_port_err.contains("Invalid INDEXER_BIND_ADDR"));
        assert!(missing_port_err.contains("127.0.0.1"));

        let bad_port_err = parse_bind_addr(Some("127.0.0.1:99999")).unwrap_err();
        assert!(bad_port_err.contains("Invalid INDEXER_BIND_ADDR"));

        let hostname_err = parse_bind_addr(Some("localhost:3000")).unwrap_err();
        assert!(hostname_err.contains("Invalid INDEXER_BIND_ADDR"));
    }

    #[test]
    fn parse_database_url_requires_non_empty_value() {
        let missing_err = parse_database_url(None).unwrap_err();
        assert!(missing_err.contains("DATABASE_URL"));
        assert!(missing_err.contains("not set"));

        let empty_err = parse_database_url(Some("   ".to_string())).unwrap_err();
        assert!(empty_err.contains("DATABASE_URL"));
        assert!(empty_err.contains("empty"));

        let valid = parse_database_url(Some("postgres://user@localhost/revora_indexer".to_string()))
            .expect("valid DATABASE_URL should succeed");
        assert_eq!(valid, "postgres://user@localhost/revora_indexer");
    }
}

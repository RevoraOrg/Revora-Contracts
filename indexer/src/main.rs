use axum::{
    extract::{Query, State},
    http::header,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use revora_indexer::{EventFilter, EventIndexer, IndexerError, NewContractEvent, Result};
use serde::Deserialize;
use std::env;
use tower_http::cors::{Any, CorsLayer};
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

#[derive(Debug, Deserialize)]
struct PaginationParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Clone)]
struct AppState {
    indexer: EventIndexer,
}

async fn get_events(
    State(state): State<AppState>,
    Query(filter): Query<EventFilter>,
    Query(pagination): Query<PaginationParams>,
) -> Response {
    match state.indexer.query_events(&filter, pagination.limit, pagination.offset).await {
        Ok(events) => (StatusCode::OK, axum::Json(events)).into_response(),
        Err(error) => IndexerErrorResponse(error).into_response(),
    }
}

async fn create_event(
    State(state): State<AppState>,
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
            [(header::CONTENT_DISPOSITION, "attachment; filename=\"events.csv\"")],
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
            [(header::CONTENT_DISPOSITION, "attachment; filename=\"events.json\"")],
            data,
        )
            .into_response(),
        Err(error) => IndexerErrorResponse(error).into_response(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();

    let database_url = env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:password@localhost/revora_indexer".to_string());
    let indexer = EventIndexer::new(&database_url).await?;
    indexer.initialize_schema().await?;

    let state = AppState { indexer };

    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);

    let app = Router::new()
        .route("/events", get(get_events).post(create_event))
        .route("/analytics", get(get_analytics))
        .route("/export/csv", get(export_csv))
        .route("/export/json", get(export_json))
        .with_state(state)
        .layer(cors);

    let addr = ([0, 0, 0, 0], 3000).into();
    tracing::info!("Listening on http://{}", addr);
    axum::Server::bind(&addr).serve(app.into_make_service()).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

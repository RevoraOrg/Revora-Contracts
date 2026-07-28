use axum::{
    extract::{Query, State},
    http::header,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use revora_indexer::{ContractEvent, EventFilter, EventIndexer, NewContractEvent, Result};
use serde::Deserialize;
use std::env;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber;

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
) -> impl IntoResponse {
    match state.indexer.query_events(&filter, pagination.limit, pagination.offset).await {
        Ok(events) => (StatusCode::OK, axum::Json(events)),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json("Failed to query events")),
    }
}

async fn create_event(
    State(state): State<AppState>,
    axum::Json(new_event): axum::Json<NewContractEvent>,
) -> impl IntoResponse {
    match state.indexer.insert_event(&new_event).await {
        Ok(event) => (StatusCode::CREATED, axum::Json(event)),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json("Failed to insert event")),
    }
}

async fn get_analytics(State(state): State<AppState>) -> impl IntoResponse {
    match state.indexer.get_analytics().await {
        Ok(analytics) => (StatusCode::OK, axum::Json(analytics)),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, axum::Json("Failed to get analytics")),
    }
}

async fn export_csv(
    State(state): State<AppState>,
    Query(filter): Query<EventFilter>,
) -> impl IntoResponse {
    match state.indexer.export_events_to_csv(&filter).await {
        Ok(data) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/csv")],
            [(header::CONTENT_DISPOSITION, "attachment; filename=\"events.csv\"")],
            data,
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain")],
            vec![],
            "Failed to export CSV".to_string().into_bytes(),
        ),
    }
}

async fn export_json(
    State(state): State<AppState>,
    Query(filter): Query<EventFilter>,
) -> impl IntoResponse {
    match state.indexer.export_events_to_json(&filter).await {
        Ok(data) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            [(header::CONTENT_DISPOSITION, "attachment; filename=\"events.json\"")],
            data,
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain")],
            vec![],
            "Failed to export JSON".to_string().into_bytes(),
        ),
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

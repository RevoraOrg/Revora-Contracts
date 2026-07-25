
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IndexerError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("CSV error: {0}")]
    CsvError(#[from] csv::Error),
    #[error("Invalid event data")]
    InvalidEventData,
}

pub type Result<T> = std::result::Result<T, IndexerError>;

/// Represents a stored smart contract event
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ContractEvent {
    pub id: Uuid,
    pub contract_address: String,
    pub event_type: String,
    pub block_number: i64,
    pub block_timestamp: DateTime<Utc>,
    pub transaction_hash: Option<String>,
    pub topics: serde_json::Value,
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Filter for querying events
#[derive(Debug, Clone, Deserialize)]
pub struct EventFilter {
    pub contract_address: Option<String>,
    pub event_type: Option<String>,
    pub block_number_min: Option<i64>,
    pub block_number_max: Option<i64>,
    pub timestamp_min: Option<DateTime<Utc>>,
    pub timestamp_max: Option<DateTime<Utc>>,
    pub topics_contains: Option<serde_json::Value>,
}

/// Event analytics result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAnalytics {
    pub total_events: i64,
    pub event_counts_by_type: serde_json::Value,
    pub event_counts_by_hour: serde_json::Value,
    pub top_contracts: serde_json::Value,
}

/// Event indexer service
pub struct EventIndexer {
    pool: sqlx::PgPool,
}

impl EventIndexer {
    /// Create a new EventIndexer instance
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = sqlx::PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    /// Initialize database schema
    pub async fn initialize_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS contract_events (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                contract_address TEXT NOT NULL,
                event_type TEXT NOT NULL,
                block_number BIGINT NOT NULL,
                block_timestamp TIMESTAMPTZ NOT NULL,
                transaction_hash TEXT,
                topics JSONB NOT NULL,
                data JSONB NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            
            CREATE INDEX IF NOT EXISTS idx_contract_events_contract_address ON contract_events(contract_address);
            CREATE INDEX IF NOT EXISTS idx_contract_events_event_type ON contract_events(event_type);
            CREATE INDEX IF NOT EXISTS idx_contract_events_block_number ON contract_events(block_number);
            CREATE INDEX IF NOT EXISTS idx_contract_events_timestamp ON contract_events(block_timestamp);
            CREATE INDEX IF NOT EXISTS idx_contract_events_topics ON contract_events USING GIN(topics);
            CREATE INDEX IF NOT EXISTS idx_contract_events_data ON contract_events USING GIN(data);
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Insert a new event
    pub async fn insert_event(&self, event: &NewContractEvent) -> Result<ContractEvent> {
        let now = Utc::now();
        let id = Uuid::new_v4();
        let row = sqlx::query_as::<_, ContractEvent>(
            r#"
            INSERT INTO contract_events (
                id, contract_address, event_type, block_number, block_timestamp,
                transaction_hash, topics, data, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(&event.contract_address)
        .bind(&event.event_type)
        .bind(event.block_number)
        .bind(event.block_timestamp)
        .bind(&event.transaction_hash)
        .bind(&event.topics)
        .bind(&event.data)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Query events with filters
    pub async fn query_events(
        &self,
        filter: &EventFilter,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<ContractEvent>> {
        let mut query = sqlx::QueryBuilder::new("SELECT * FROM contract_events WHERE 1=1");

        if let Some(contract_address) = &filter.contract_address {
            query.push(" AND contract_address = ");
            query.push_bind(contract_address);
        }

        if let Some(event_type) = &filter.event_type {
            query.push(" AND event_type = ");
            query.push_bind(event_type);
        }

        if let Some(min) = filter.block_number_min {
            query.push(" AND block_number >= ");
            query.push_bind(min);
        }

        if let Some(max) = filter.block_number_max {
            query.push(" AND block_number <= ");
            query.push_bind(max);
        }

        if let Some(min) = filter.timestamp_min {
            query.push(" AND block_timestamp >= ");
            query.push_bind(min);
        }

        if let Some(max) = filter.timestamp_max {
            query.push(" AND block_timestamp <= ");
            query.push_bind(max);
        }

        query.push(" ORDER BY block_timestamp DESC");

        if let Some(limit) = limit {
            query.push(" LIMIT ");
            query.push_bind(limit);
        }

        if let Some(offset) = offset {
            query.push(" OFFSET ");
            query.push_bind(offset);
        }

        let events = query.build_query_as().fetch_all(&self.pool).await?;
        Ok(events)
    }

    /// Get event analytics
    pub async fn get_analytics(&self) -> Result<EventAnalytics> {
        let total_events = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM contract_events")
            .fetch_one(&self.pool)
            .await?;

        let event_counts_by_type: Vec<(String, i64)> = sqlx::query_as(
            "SELECT event_type, COUNT(*) as count FROM contract_events GROUP BY event_type")
            .fetch_all(&self.pool)
            .await?;
        let event_counts_by_type = serde_json::to_value(event_counts_by_type)?;

        let event_counts_by_hour: Vec<(DateTime<Utc>, i64)> = sqlx::query_as(
            "SELECT date_trunc('hour', block_timestamp) as hour, COUNT(*) as count 
         FROM contract_events 
         GROUP BY hour 
         ORDER BY hour DESC 
         LIMIT 24"
        )
        .fetch_all(&self.pool)
        .await?;
        let event_counts_by_hour = serde_json::to_value(event_counts_by_hour)?;

        let top_contracts: Vec<(String, i64)> = sqlx::query_as(
            "SELECT contract_address, COUNT(*) as count FROM contract_events GROUP BY contract_address ORDER BY count DESC LIMIT 10"
        )
        .fetch_all(&self.pool)
        .await?;
        let top_contracts = serde_json::to_value(top_contracts)?;

        Ok(EventAnalytics {
            total_events,
            event_counts_by_type,
            event_counts_by_hour,
            top_contracts,
        })
    }

    /// Export events to CSV
    pub async fn export_events_to_csv(&self, filter: &EventFilter) -> Result<Vec<u8>> {
        let events = self.query_events(filter, None, None).await?;
        let mut wtr = csv::Writer::from_writer(vec![]);

        for event in events {
            wtr.serialize(event)?;
        }

        wtr.flush()?;
        Ok(wtr.into_inner()?)
    }

    /// Export events to JSON
    pub async fn export_events_to_json(&self, filter: &EventFilter) -> Result<Vec<u8>> {
        let events = self.query_events(filter, None, None).await?;
        Ok(serde_json::to_vec(&events)?)
    }
}

/// New event for insertion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewContractEvent {
    pub contract_address: String,
    pub event_type: String,
    pub block_number: i64,
    pub block_timestamp: DateTime<Utc>,
    pub transaction_hash: Option<String>,
    pub topics: serde_json::Value,
    pub data: serde_json::Value,
}

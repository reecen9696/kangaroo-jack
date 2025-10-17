use crate::types::{CoinflipRequest, CoinflipResponse, VfError};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use std::sync::Arc;
use tracing::{info, error};

pub struct Storage {
    pool: SqlitePool,
}

impl Storage {
    pub async fn new(database_url: &str) -> Result<Self, VfError> {
        info!("🗄️  Initializing database connection: {}", database_url);

        // Configure SQLite connection
        let options = SqliteConnectOptions::new()
            .filename(database_url.strip_prefix("sqlite:").unwrap_or(database_url))
            .create_if_missing(true);

        let pool = SqlitePool::connect_with(options).await?;

        // Run migrations
        Self::run_migrations(&pool).await?;

        info!("✅ Database initialized successfully");

        Ok(Self { pool })
    }

    pub fn pool(&self) -> Arc<SqlitePool> {
        Arc::new(self.pool.clone())
    }

    async fn run_migrations(pool: &SqlitePool) -> Result<(), VfError> {
        info!("🔄 Running database migrations...");

        // Create pending_bets table
        sqlx::query!(
            r#"
            CREATE TABLE IF NOT EXISTS pending_bets (
                bet_id TEXT PRIMARY KEY,
                user_seed TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                node_id TEXT NOT NULL,
                heads BOOLEAN NOT NULL,
                vrf_proof TEXT NOT NULL,
                processing_time_ms INTEGER NOT NULL,
                processed_at TEXT NOT NULL,
                retry_count INTEGER DEFAULT 0,
                status TEXT DEFAULT 'pending',
                tx_signature TEXT NULL,
                settled_at TEXT NULL,
                failed_at TEXT NULL,
                error_message TEXT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#
        )
        .execute(pool)
        .await?;

        // Create settlement_batches table
        sqlx::query!(
            r#"
            CREATE TABLE IF NOT EXISTS settlement_batches (
                batch_id TEXT PRIMARY KEY,
                bet_count INTEGER NOT NULL,
                processing_time_ms INTEGER NOT NULL,
                tx_signature TEXT NOT NULL,
                success BOOLEAN NOT NULL,
                created_at TEXT NOT NULL
            )
            "#
        )
        .execute(pool)
        .await?;

        // Create indexes
        sqlx::query!("CREATE INDEX IF NOT EXISTS idx_pending_bets_status ON pending_bets(status)")
            .execute(pool)
            .await?;

        sqlx::query!("CREATE INDEX IF NOT EXISTS idx_pending_bets_processed_at ON pending_bets(processed_at)")
            .execute(pool)
            .await?;

        sqlx::query!("CREATE INDEX IF NOT EXISTS idx_pending_bets_retry_count ON pending_bets(retry_count)")
            .execute(pool)
            .await?;

        sqlx::query!("CREATE INDEX IF NOT EXISTS idx_settlement_batches_created_at ON settlement_batches(created_at)")
            .execute(pool)
            .await?;

        sqlx::query!("CREATE INDEX IF NOT EXISTS idx_settlement_batches_success ON settlement_batches(success)")
            .execute(pool)
            .await?;

        info!("✅ Database migrations completed");
        Ok(())
    }

    /// Store bet result (optional - for audit trail)
    pub async fn store_bet(
        &self,
        request: &CoinflipRequest,
        response: &CoinflipResponse,
    ) -> Result<(), VfError> {
        sqlx::query!(
            r#"
            INSERT INTO bet_results (
                user_seed, timestamp, node_id, heads, 
                vrf_proof, processing_time_ms, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, datetime('now'))
            "#,
            request.user_seed,
            request.timestamp as i64,
            response.node_id,
            response.heads,
            response.proof.signature,
            response.processing_time_ms as i64
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to store bet result");
            VfError::InvalidInput(format!("Database error: {}", e))
        })?;

        Ok(())
    }

    /// Get settlement statistics from database
    pub async fn get_settlement_summary(&self) -> Result<serde_json::Value, VfError> {
        let stats = sqlx::query!(
            r#"
            SELECT 
                COUNT(*) as total_bets,
                SUM(CASE WHEN status = 'settled' THEN 1 ELSE 0 END) as settled_bets,
                SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending_bets,
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed_bets,
                AVG(CASE WHEN status = 'settled' THEN processing_time_ms END) as avg_processing_time
            FROM pending_bets
            "#
        )
        .fetch_one(&self.pool)
        .await?;

        let batch_stats = sqlx::query!(
            r#"
            SELECT 
                COUNT(*) as total_batches,
                SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful_batches,
                AVG(bet_count) as avg_batch_size,
                AVG(processing_time_ms) as avg_batch_processing_time
            FROM settlement_batches
            "#
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(serde_json::json!({
            "bets": {
                "total": stats.total_bets,
                "settled": stats.settled_bets,
                "pending": stats.pending_bets,
                "failed": stats.failed_bets,
                "avg_processing_time_ms": stats.avg_processing_time
            },
            "batches": {
                "total": batch_stats.total_batches,
                "successful": batch_stats.successful_batches,
                "avg_size": batch_stats.avg_batch_size,
                "avg_processing_time_ms": batch_stats.avg_batch_processing_time
            }
        }))
    }
}

impl From<sqlx::Error> for VfError {
    fn from(err: sqlx::Error) -> Self {
        VfError::InvalidInput(format!("Database error: {}", err))
    }
}

impl From<uuid::Error> for VfError {
    fn from(err: uuid::Error) -> Self {
        VfError::InvalidInput(format!("UUID error: {}", err))
    }
}

impl From<time::error::Parse> for VfError {
    fn from(err: time::error::Parse) -> Self {
        VfError::InvalidInput(format!("Time parsing error: {}", err))
    }
}
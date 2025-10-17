use crate::types::{CoinflipRequest, CoinflipResponse, VfError};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use std::collections::VecDeque;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingBet {
    pub bet_id: Uuid,
    pub user_seed: String,
    pub timestamp: u64,
    pub node_id: String,
    pub heads: bool,
    pub vrf_proof: String,
    pub processing_time_ms: u64,
    pub processed_at: time::OffsetDateTime,
    pub retry_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettlementBatch {
    pub batch_id: Uuid,
    pub bets: Vec<PendingBet>,
    pub bet_count: usize,
    pub created_at: time::OffsetDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchResult {
    pub batch_id: Uuid,
    pub success: bool,
    pub processed_count: usize,
    pub processing_time_ms: u64,
    pub mock_tx_signature: String,
    pub timestamp: time::OffsetDateTime,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct SettlementStats {
    pub total_bets_processed: u64,
    pub total_batches_processed: u64,
    pub successful_batches: u64,
    pub failed_batches: u64,
    pub average_batch_size: f64,
    pub average_processing_time_ms: f64,
    pub last_settlement_time: Option<time::OffsetDateTime>,
    pub current_queue_size: usize,
    pub retry_queue_size: usize,
    pub channel_queue_size: usize,
}

pub struct SettlementEngine {
    // High-performance async channel for instant enqueuing
    bet_sender: mpsc::UnboundedSender<PendingBet>,
    
    // Background processing state
    db_pool: Arc<SqlitePool>,
    retry_queue: Arc<Mutex<VecDeque<PendingBet>>>,
    stats: Arc<RwLock<SettlementStats>>,
    
    // Configuration
    batch_size: usize,
    max_retries: u32,
    processing_interval_seconds: u64,
}

impl SettlementEngine {
    pub fn new(
        db_pool: Arc<SqlitePool>,
        batch_size: usize,
        processing_interval_seconds: u64,
    ) -> Result<Arc<Self>, VfError> {
        let (bet_sender, bet_receiver) = mpsc::unbounded_channel();
        
        let engine = Arc::new(Self {
            bet_sender,
            db_pool: db_pool.clone(),
            retry_queue: Arc::new(Mutex::new(VecDeque::new())),
            stats: Arc::new(RwLock::new(SettlementStats::default())),
            batch_size,
            max_retries: 3,
            processing_interval_seconds,
        });

        // Start background processors
        Self::start_background_processors(engine.clone(), bet_receiver);
        
        Ok(engine)
    }

    /// INSTANT: Add bet to settlement queue (no blocking I/O)
    pub fn enqueue_bet_fast(&self, bet_response: &CoinflipResponse, request: &CoinflipRequest) -> Result<(), VfError> {
        let pending_bet = PendingBet {
            bet_id: Uuid::new_v4(), // Generate new ID for settlement tracking
            user_seed: request.user_seed.clone(),
            timestamp: request.timestamp,
            node_id: bet_response.node_id.clone(),
            heads: bet_response.heads,
            vrf_proof: bet_response.proof.signature.clone(),
            processing_time_ms: bet_response.processing_time_ms,
            processed_at: time::OffsetDateTime::now_utc(),
            retry_count: 0,
        };

        // ⚡ INSTANT: Send to channel (microseconds)
        self.bet_sender
            .send(pending_bet)
            .map_err(|_| VfError::InvalidInput("Settlement channel closed".to_string()))?;

        debug!(
            user_seed = %request.user_seed,
            heads = bet_response.heads,
            "✅ Bet enqueued instantly"
        );

        Ok(())
    }

    /// Start all background processing tasks
    fn start_background_processors(
        engine: Arc<Self>,
        mut bet_receiver: mpsc::UnboundedReceiver<PendingBet>,
    ) {
        // Background task 1: Drain channel to database
        let engine_db = engine.clone();
        tokio::spawn(async move {
            let mut batch_buffer = Vec::new();
            let mut last_flush = std::time::Instant::now();
            
            loop {
                // Collect bets from channel
                while let Ok(bet) = bet_receiver.try_recv() {
                    batch_buffer.push(bet);
                    
                    // Batch writes for efficiency (100 bets or 10ms timeout)
                    if batch_buffer.len() >= 100 || last_flush.elapsed().as_millis() > 10 {
                        if let Err(e) = engine_db.flush_batch_to_db(&batch_buffer).await {
                            error!(error = %e, "Failed to flush batch to database");
                        }
                        batch_buffer.clear();
                        last_flush = std::time::Instant::now();
                    }
                }
                
                // Periodic flush for remaining bets
                if !batch_buffer.is_empty() && last_flush.elapsed().as_millis() > 10 {
                    if let Err(e) = engine_db.flush_batch_to_db(&batch_buffer).await {
                        error!(error = %e, "Failed to flush remaining batch to database");
                    }
                    batch_buffer.clear();
                    last_flush = std::time::Instant::now();
                }
                
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
            }
        });

        // Background task 2: Settlement processing loop
        let engine_settlement = engine.clone();
        tokio::spawn(async move {
            if let Err(e) = engine_settlement.run_settlement_loop().await {
                error!(error = %e, "Settlement loop crashed");
            }
        });

        // Background task 3: Stats printing
        let engine_stats = engine.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                engine_stats.print_stats().await;
            }
        });

        info!("🚀 Settlement engine background processors started");
    }

    /// Flush accumulated bets to database (batched for efficiency)
    async fn flush_batch_to_db(&self, batch: &[PendingBet]) -> Result<(), VfError> {
        if batch.is_empty() {
            return Ok(());
        }

        let start = std::time::Instant::now();

        // Begin transaction for batch insert
        let mut tx = self.db_pool.begin().await?;
        
        for bet in batch {
            sqlx::query!(
                r#"
                INSERT INTO pending_bets (
                    bet_id, user_seed, timestamp, node_id, heads, 
                    vrf_proof, processing_time_ms, processed_at, retry_count, status
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending')
                "#,
                bet.bet_id.to_string(),
                bet.user_seed,
                bet.timestamp as i64,
                bet.node_id,
                bet.heads,
                bet.vrf_proof,
                bet.processing_time_ms as i64,
                bet.processed_at.format(&time::format_description::well_known::Rfc3339).unwrap(),
                bet.retry_count as i32
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        debug!(
            batch_size = batch.len(),
            flush_time_ms = start.elapsed().as_millis(),
            "💾 Flushed bet batch to database"
        );

        Ok(())
    }

    /// Main settlement processing loop (runs periodically)
    async fn run_settlement_loop(&self) -> Result<(), VfError> {
        info!(
            interval_seconds = self.processing_interval_seconds,
            batch_size = self.batch_size,
            "🔄 Starting settlement processing loop"
        );

        let mut interval = tokio::time::interval(
            tokio::time::Duration::from_secs(self.processing_interval_seconds)
        );

        // Load any pending bets from database on startup (crash recovery)
        self.load_pending_bets_from_db().await?;

        loop {
            interval.tick().await;
            
            if let Err(e) = self.process_settlement_batch().await {
                error!(error = %e, "❌ Settlement batch processing failed");
            }
        }
    }

    /// Process one settlement batch
    async fn process_settlement_batch(&self) -> Result<(), VfError> {
        let start_time = std::time::Instant::now();

        // 1. Collect bets for this batch from database
        let batch = self.collect_batch_from_db().await?;
        
        if batch.is_empty() {
            debug!("📭 No bets to settle this round");
            return Ok(());
        }

        let batch_id = Uuid::new_v4();

        info!(
            batch_id = %batch_id,
            batch_size = batch.len(),
            heads_count = batch.iter().filter(|b| b.heads).count(),
            tails_count = batch.iter().filter(|b| !b.heads).count(),
            "🎯 Processing settlement batch"
        );

        // 2. Create settlement batch
        let settlement_batch = SettlementBatch {
            batch_id,
            bets: batch.clone(),
            bet_count: batch.len(),
            created_at: time::OffsetDateTime::now_utc(),
        };

        // 3. Mock settlement processing (will be replaced with Solana logic)
        let result = self.mock_settle_batch(&settlement_batch).await;

        let processing_time = start_time.elapsed();

        match result {
            Ok(mock_tx_signature) => {
                let batch_result = BatchResult {
                    batch_id,
                    success: true,
                    processed_count: batch.len(),
                    processing_time_ms: processing_time.as_millis() as u64,
                    mock_tx_signature,
                    timestamp: time::OffsetDateTime::now_utc(),
                };

                // Mark as settled in database
                self.mark_batch_settled(&batch, &batch_result).await?;
                self.update_stats_success(&batch_result).await;

                info!(
                    batch_id = %batch_id,
                    tx_signature = %batch_result.mock_tx_signature,
                    processing_ms = processing_time.as_millis(),
                    "✅ Settlement batch completed successfully"
                );
            }
            Err(e) => {
                error!(
                    batch_id = %batch_id,
                    error = %e,
                    processing_ms = processing_time.as_millis(),
                    "❌ Settlement batch failed"
                );

                self.handle_batch_failure(batch, e).await?;
                self.update_stats_failure().await;
            }
        }

        Ok(())
    }

    /// Collect pending bets from database for settlement
    async fn collect_batch_from_db(&self) -> Result<Vec<PendingBet>, VfError> {
        let mut batch = Vec::new();

        // First, get retries from in-memory queue (higher priority)
        {
            let mut retry_queue = self.retry_queue.lock().await;
            while batch.len() < self.batch_size && !retry_queue.is_empty() {
                if let Some(bet) = retry_queue.pop_front() {
                    debug!(
                        bet_id = %bet.bet_id,
                        retry_count = bet.retry_count,
                        "🔄 Adding retry bet to batch"
                    );
                    batch.push(bet);
                }
            }
        }

        // Then, get pending bets from database
        if batch.len() < self.batch_size {
            let remaining_capacity = self.batch_size - batch.len();
            
            let rows = sqlx::query!(
                "SELECT * FROM pending_bets WHERE status = 'pending' ORDER BY processed_at ASC LIMIT ?",
                remaining_capacity as i32
            )
            .fetch_all(&*self.db_pool)
            .await?;

            for row in rows {
                let bet = PendingBet {
                    bet_id: Uuid::parse_str(&row.bet_id)?,
                    user_seed: row.user_seed,
                    timestamp: row.timestamp as u64,
                    node_id: row.node_id,
                    heads: row.heads,
                    vrf_proof: row.vrf_proof,
                    processing_time_ms: row.processing_time_ms as u64,
                    processed_at: time::OffsetDateTime::parse(
                        &row.processed_at, 
                        &time::format_description::well_known::Rfc3339
                    )?,
                    retry_count: row.retry_count as u32,
                };
                batch.push(bet);
            }
        }

        if !batch.is_empty() {
            debug!(
                batch_size = batch.len(),
                oldest_bet_age_seconds = {
                    let now = time::OffsetDateTime::now_utc();
                    let oldest = batch.first().unwrap();
                    (now - oldest.processed_at).whole_seconds()
                },
                "📦 Batch collected from database"
            );
        }

        Ok(batch)
    }

    /// Mock settlement (will be replaced with Solana transaction)
    async fn mock_settle_batch(&self, batch: &SettlementBatch) -> Result<String, VfError> {
        // Simulate processing time based on batch size
        tokio::time::sleep(tokio::time::Duration::from_millis(50 + batch.bet_count as u64 * 2)).await;

        // Simulate occasional failures (2% failure rate for testing)
        if rand::random::<f64>() < 0.02 {
            return Err(VfError::InvalidInput("Mock settlement timeout".to_string()));
        }

        // Generate mock transaction signature
        let mock_tx_signature = format!("mock_settlement_{}", Uuid::new_v4().simple());

        debug!(
            batch_id = %batch.batch_id,
            mock_tx_signature = %mock_tx_signature,
            bet_count = batch.bet_count,
            "🎲 Mock settlement transaction processed"
        );

        Ok(mock_tx_signature)
    }

    /// Handle batch settlement failure
    async fn handle_batch_failure(&self, batch: Vec<PendingBet>, error: VfError) -> Result<(), VfError> {
        let mut retryable = Vec::new();
        let mut permanently_failed = Vec::new();

        for mut bet in batch {
            bet.retry_count += 1;

            if bet.retry_count <= self.max_retries {
                retryable.push(bet);
            } else {
                permanently_failed.push(bet);
            }
        }

        // Add retryable bets to retry queue
        if !retryable.is_empty() {
            let mut retry_queue = self.retry_queue.lock().await;
            for bet in &retryable {
                retry_queue.push_back(bet.clone());
            }

            warn!(
                retry_count = retryable.len(),
                "🔄 Bets added to retry queue"
            );
        }

        // Mark permanently failed bets in database
        if !permanently_failed.is_empty() {
            for bet in &permanently_failed {
                self.mark_bet_permanently_failed(bet, &error.to_string()).await?;
            }

            error!(
                failed_count = permanently_failed.len(),
                "💀 Bets permanently failed after max retries"
            );
        }

        Ok(())
    }

    /// Load pending bets from database on startup (crash recovery)
    async fn load_pending_bets_from_db(&self) -> Result<(), VfError> {
        let rows = sqlx::query!(
            "SELECT * FROM pending_bets WHERE status = 'pending' AND retry_count > 0 ORDER BY processed_at ASC"
        )
        .fetch_all(&*self.db_pool)
        .await?;

        if !rows.is_empty() {
            let mut retry_queue = self.retry_queue.lock().await;
            
            for row in &rows {
                let bet = PendingBet {
                    bet_id: Uuid::parse_str(&row.bet_id)?,
                    user_seed: row.user_seed.clone(),
                    timestamp: row.timestamp as u64,
                    node_id: row.node_id.clone(),
                    heads: row.heads,
                    vrf_proof: row.vrf_proof.clone(),
                    processing_time_ms: row.processing_time_ms as u64,
                    processed_at: time::OffsetDateTime::parse(
                        &row.processed_at, 
                        &time::format_description::well_known::Rfc3339
                    )?,
                    retry_count: row.retry_count as u32,
                };
                retry_queue.push_back(bet);
            }

            info!(
                retry_loaded = rows.len(),
                "🔄 Loaded retry bets from database"
            );
        }

        Ok(())
    }

    /// Mark batch as settled in database
    async fn mark_batch_settled(&self, batch: &[PendingBet], result: &BatchResult) -> Result<(), VfError> {
        let mut tx = self.db_pool.begin().await?;

        // Update bet statuses
        for bet in batch {
            sqlx::query!(
                "UPDATE pending_bets SET status = 'settled', tx_signature = ?, settled_at = ? WHERE bet_id = ?",
                result.mock_tx_signature,
                result.timestamp.format(&time::format_description::well_known::Rfc3339).unwrap(),
                bet.bet_id.to_string()
            )
            .execute(&mut *tx)
            .await?;
        }

        // Store batch result
        sqlx::query!(
            r#"
            INSERT INTO settlement_batches (
                batch_id, bet_count, processing_time_ms, 
                tx_signature, success, created_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
            result.batch_id.to_string(),
            result.processed_count as i32,
            result.processing_time_ms as i64,
            result.mock_tx_signature,
            result.success,
            result.timestamp.format(&time::format_description::well_known::Rfc3339).unwrap()
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    /// Mark bet as permanently failed
    async fn mark_bet_permanently_failed(&self, bet: &PendingBet, error: &str) -> Result<(), VfError> {
        sqlx::query!(
            "UPDATE pending_bets SET status = 'failed', error_message = ?, failed_at = ? WHERE bet_id = ?",
            error,
            time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap(),
            bet.bet_id.to_string()
        )
        .execute(&*self.db_pool)
        .await?;

        Ok(())
    }

    /// Get current settlement statistics
    pub async fn get_stats(&self) -> SettlementStats {
        let mut stats = self.stats.read().await.clone();
        
        // Update current queue sizes
        stats.retry_queue_size = {
            let queue = self.retry_queue.lock().await;
            queue.len()
        };

        // Estimate channel queue size (can't get exact size from UnboundedReceiver)
        stats.channel_queue_size = 0; // This will be updated by background processor

        stats
    }

    /// Print detailed stats
    pub async fn print_stats(&self) {
        let stats = self.get_stats().await;
        
        info!("📊 SETTLEMENT ENGINE STATS");
        info!("   Total Bets Processed: {}", stats.total_bets_processed);
        info!(
            "   Total Batches: {} (✅ {} successful, ❌ {} failed)",
            stats.total_batches_processed, stats.successful_batches, stats.failed_batches
        );
        info!("   Average Batch Size: {:.1}", stats.average_batch_size);
        info!("   Average Processing Time: {:.1}ms", stats.average_processing_time_ms);
        info!("   Current Queues: {} retries", stats.retry_queue_size);
        
        if let Some(last_time) = stats.last_settlement_time {
            info!(
                "   Last Settlement: {} seconds ago",
                (time::OffsetDateTime::now_utc() - last_time).whole_seconds()
            );
        }
    }

    /// Update stats on successful batch
    async fn update_stats_success(&self, result: &BatchResult) {
        let mut stats = self.stats.write().await;
        
        stats.total_bets_processed += result.processed_count as u64;
        stats.total_batches_processed += 1;
        stats.successful_batches += 1;
        stats.last_settlement_time = Some(result.timestamp);
        
        // Update running averages
        if stats.total_batches_processed > 0 {
            stats.average_batch_size = stats.total_bets_processed as f64 / stats.total_batches_processed as f64;
            stats.average_processing_time_ms = (
                (stats.average_processing_time_ms * (stats.total_batches_processed - 1) as f64) +
                result.processing_time_ms as f64
            ) / stats.total_batches_processed as f64;
        }
    }

    /// Update stats on failed batch
    async fn update_stats_failure(&self) {
        let mut stats = self.stats.write().await;
        stats.total_batches_processed += 1;
        stats.failed_batches += 1;
    }
}

// Implement From trait for easy conversion
impl From<&CoinflipResponse> for PendingBet {
    fn from(response: &CoinflipResponse) -> Self {
        Self {
            bet_id: Uuid::new_v4(),
            user_seed: "extracted_from_request".to_string(), // Will be properly extracted
            timestamp: response.timestamp,
            node_id: response.node_id.clone(),
            heads: response.heads,
            vrf_proof: response.proof.signature.clone(),
            processing_time_ms: response.processing_time_ms,
            processed_at: time::OffsetDateTime::now_utc(),
            retry_count: 0,
        }
    }
}
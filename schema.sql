-- Settlement Engine Database Schema
-- Tables for managing bet settlement processing

-- Table to store pending bets awaiting settlement
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
    status TEXT DEFAULT 'pending', -- 'pending', 'settling', 'settled', 'failed'
    tx_signature TEXT NULL,
    settled_at TEXT NULL,
    failed_at TEXT NULL,
    error_message TEXT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Table to store settlement batch results
CREATE TABLE IF NOT EXISTS settlement_batches (
    batch_id TEXT PRIMARY KEY,
    bet_count INTEGER NOT NULL,
    processing_time_ms INTEGER NOT NULL,
    tx_signature TEXT NOT NULL,
    success BOOLEAN NOT NULL,
    created_at TEXT NOT NULL
);

-- Indexes for efficient querying
CREATE INDEX IF NOT EXISTS idx_pending_bets_status ON pending_bets(status);
CREATE INDEX IF NOT EXISTS idx_pending_bets_processed_at ON pending_bets(processed_at);
CREATE INDEX IF NOT EXISTS idx_pending_bets_retry_count ON pending_bets(retry_count);
CREATE INDEX IF NOT EXISTS idx_settlement_batches_created_at ON settlement_batches(created_at);
CREATE INDEX IF NOT EXISTS idx_settlement_batches_success ON settlement_batches(success);
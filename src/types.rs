use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinflipRequest {
    #[serde(alias = "seed")]
    pub user_seed: String,
    #[serde(default = "default_timestamp")]
    pub timestamp: u64,
}

fn default_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinflipResponse {
    pub node_id: String,
    pub heads: bool,
    pub proof: VrfProof,
    pub timestamp: u64, // Unix timestamp
    pub processing_time_ms: u64, // Performance metric
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VrfProof {
    pub seed_commitment: String, // Base64 seed commitment
    pub vrf_output: String,      // Base64 VRF output
    pub signature: String,       // Base64 signature
}

#[derive(Debug, thiserror::Error)]
pub enum VfError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Invalid proof: {0}")]
    InvalidProof(String),
    #[error("VRF generation failed: {0}")]
    VrfFailed(String),
}
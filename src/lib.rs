pub mod types;
pub mod vrf_engine;

pub use types::*;
pub use vrf_engine::VrfEngine;

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_coinflip_deterministic() {
        // Use deterministic seed for reproducible test
        let seed = [42u8; 32];
        let engine = VrfEngine::from_seed(seed);
        
        let bet = CoinflipRequest {
            bet_id: Uuid::new_v4(),
            wager_lamports: 1_000_000, // 0.001 SOL
            token_mint: "SOL".to_string(),
            player_pubkey: "11111111111111111111111111111112".to_string(),
            client_seed: Some("deadbeef".to_string()),
            wallet_sig: "test_signature".to_string(),
            ts_ms: OffsetDateTime::now_utc(),
        };

        let result = engine.process_coinflip(&bet).expect("Coinflip should succeed");
        
        // Verify basic structure
        assert_eq!(result.bet_id, bet.bet_id);
        assert!(result.payout_lamports == 0 || result.payout_lamports == 2_000_000);
        assert!(!result.proof.vrf_proof.is_empty());
        assert!(!result.proof.node_signature.is_empty());
        assert!(!result.proof.seed_commit.is_empty());
        
        // Verify the result can be verified
        assert!(engine.verify_result(&bet, &result).expect("Verification should succeed"));
        
        // Test determinism - same input should give same output
        let result2 = engine.process_coinflip(&bet).expect("Second coinflip should succeed");
        assert_eq!(result.heads, result2.heads);
        assert_eq!(result.win, result2.win);
        assert_eq!(result.payout_lamports, result2.payout_lamports);
    }

    #[test]
    fn test_validation_errors() {
        let engine = VrfEngine::new();
        
        // Test empty wager
        let mut bet = CoinflipRequest {
            bet_id: Uuid::new_v4(),
            wager_lamports: 0, // Invalid!
            token_mint: "SOL".to_string(),
            player_pubkey: "11111111111111111111111111111112".to_string(),
            client_seed: None,
            wallet_sig: "test_sig".to_string(),
            ts_ms: OffsetDateTime::now_utc(),
        };

        assert!(matches!(engine.process_coinflip(&bet), Err(VfError::Invalid(_))));
        
        // Test old timestamp
        bet.wager_lamports = 1_000_000;
        bet.ts_ms = OffsetDateTime::now_utc() - time::Duration::minutes(2); // Too old
        
        assert!(matches!(engine.process_coinflip(&bet), Err(VfError::InvalidTimestamp)));
        
        // Test empty signature
        bet.ts_ms = OffsetDateTime::now_utc();
        bet.wallet_sig = "".to_string(); // Invalid!
        
        assert!(matches!(engine.process_coinflip(&bet), Err(VfError::Invalid(_))));
    }

    #[test]
    fn test_game_logic() {
        let engine = VrfEngine::new();
        
        let bet = CoinflipRequest {
            bet_id: Uuid::new_v4(),
            wager_lamports: 1_000_000,
            token_mint: "SOL".to_string(),
            player_pubkey: "11111111111111111111111111111112".to_string(),
            client_seed: Some("test".to_string()),
            wallet_sig: "test_sig".to_string(),
            ts_ms: OffsetDateTime::now_utc(),
        };

        let result = engine.process_coinflip(&bet).expect("Coinflip should succeed");
        
        // In our MVP, player always picks heads and wins if result is heads
        assert_eq!(result.win, result.heads);
        
        // Payout should be double wager if win, zero if lose
        let expected_payout = if result.win { bet.wager_lamports * 2 } else { 0 };
        assert_eq!(result.payout_lamports, expected_payout);
    }

    #[test]
    fn test_vrf_proof_format() {
        let engine = VrfEngine::new();
        
        let bet = CoinflipRequest {
            bet_id: Uuid::new_v4(),
            wager_lamports: 1_000_000,
            token_mint: "SOL".to_string(),
            player_pubkey: "11111111111111111111111111111112".to_string(),
            client_seed: None,
            wallet_sig: "test_sig".to_string(),
            ts_ms: OffsetDateTime::now_utc(),
        };

        let result = engine.process_coinflip(&bet).expect("Coinflip should succeed");
        
        // Verify proof formats
        assert!(hex::decode(&result.proof.vrf_proof).is_ok(), "VRF proof should be valid hex");
        assert!(hex::decode(&result.proof.seed_commit).is_ok(), "Seed commit should be valid hex");
        assert!(hex::decode(&result.proof.random_value).is_ok(), "Random value should be valid hex");
        assert!(base64::decode(&result.proof.node_signature).is_ok(), "Signature should be valid base64");
        assert!(base64::decode(&result.proof.node_pubkey).is_ok(), "Pubkey should be valid base64");
        
        // Verify random value is 16 hex chars (8 bytes)
        assert_eq!(result.proof.random_value.len(), 16);
    }
}
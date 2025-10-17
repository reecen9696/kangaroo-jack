use crate::types::{CoinflipRequest, CoinflipResponse, VrfProof, VfError};
use ed25519_dalek::{SigningKey, Signature, Signer, VerifyingKey, Verifier};
use merlin::Transcript;
use rand::{thread_rng, RngCore};
use base64::{Engine as _, engine::general_purpose::STANDARD as Base64Engine};
use sha2::{Sha256, Digest};

pub struct VrfEngine {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl VrfEngine {
    pub fn new() -> Self {
        let mut csprng = thread_rng();
        let mut secret_bytes = [0u8; 32];
        csprng.fill_bytes(&mut secret_bytes);
        let signing_key = SigningKey::from_bytes(&secret_bytes);
        let verifying_key = signing_key.verifying_key();
        
        Self { signing_key, verifying_key }
    }

    /// Create VRF engine with deterministic keypair (for testing)
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        Self { signing_key, verifying_key }
    }

    pub fn node_pubkey(&self) -> String {
        Base64Engine.encode(self.verifying_key.as_bytes())
    }

    // Optimized for high performance - no async overhead for CPU-bound work
    #[inline]
    pub fn process_coinflip(&self, req: &CoinflipRequest) -> Result<CoinflipResponse, VfError> {
        let start_time = std::time::Instant::now();

        // 1. Fast validation
        self.validate_request(req)?;

        // 2. Build transcript (optimized)
        let transcript = self.build_transcript(req);

        // 3. Generate VRF (CPU-intensive, but fast)
        let (random_value, vrf_proof_bytes, seed_commit) = self.generate_vrf(&transcript)?;

        // 4. Game logic (branchless for speed)
        let heads = random_value & 1 == 0; // Even = heads, odd = tails

        // 5. Create proof structure
        let proof = VrfProof {
            seed_commitment: seed_commit,
            vrf_output: Base64Engine.encode(&random_value.to_le_bytes()),
            signature: Base64Engine.encode(&vrf_proof_bytes),
        };

        let processing_time = start_time.elapsed().as_millis() as u64;

        Ok(CoinflipResponse {
            node_id: self.node_pubkey(),
            heads,
            proof,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            processing_time_ms: processing_time,
        })
    }

    #[inline]
    fn validate_request(&self, req: &CoinflipRequest) -> Result<(), VfError> {
        if req.user_seed.is_empty() {
            return Err(VfError::InvalidInput("User seed cannot be empty".to_string()));
        }
        if req.user_seed.len() > 1024 {
            return Err(VfError::InvalidInput("User seed too long".to_string()));
        }
        Ok(())
    }

    #[inline]
    fn build_transcript(&self, req: &CoinflipRequest) -> Transcript {
        let mut transcript = Transcript::new(b"vf_coinflip");
        transcript.append_message(b"user_seed", req.user_seed.as_bytes());
        transcript.append_message(b"node_pubkey", self.verifying_key.as_bytes());
        transcript.append_u64(b"timestamp", req.timestamp);
        transcript
    }

    #[inline]
    fn generate_vrf(&self, transcript: &Transcript) -> Result<(u64, Vec<u8>, String), VfError> {
        let mut hash_transcript = transcript.clone();
        
        // Create seed commitment
        let mut hasher = Sha256::new();
        hasher.update(self.verifying_key.as_bytes());
        let seed_commit = hasher.finalize();
        let seed_commit_str = Base64Engine.encode(&seed_commit);
        
        // Generate random value using VRF-like construction
        hash_transcript.append_message(b"seed_commit", &seed_commit);
        
        // Challenge
        let mut challenge_bytes = [0u8; 64];
        hash_transcript.challenge_bytes(b"challenge", &mut challenge_bytes);
        
        // Sign the challenge
        let signature = self.signing_key.sign(&challenge_bytes);
        
        // Derive random value from signature (deterministic)
        let mut output_hasher = Sha256::new();
        output_hasher.update(signature.to_bytes());
        let output_hash = output_hasher.finalize();
        
        // Convert to u64 for game logic
        let mut value_bytes = [0u8; 8];
        value_bytes.copy_from_slice(&output_hash[..8]);
        let random_value = u64::from_le_bytes(value_bytes);
        
        Ok((random_value, signature.to_bytes().to_vec(), seed_commit_str))
    }

    pub fn verify_proof(&self, proof: &VrfProof, req: &CoinflipRequest) -> Result<bool, VfError> {
        // Rebuild transcript
        let transcript = self.build_transcript(req);
        
        // Decode proof components
        let seed_commit = Base64Engine.decode(&proof.seed_commitment)
            .map_err(|_| VfError::InvalidProof("Invalid seed commitment encoding".to_string()))?;
        
        let signature_bytes = Base64Engine.decode(&proof.signature)
            .map_err(|_| VfError::InvalidProof("Invalid signature encoding".to_string()))?;
        
        if signature_bytes.len() != 64 {
            return Err(VfError::InvalidProof("Invalid signature length".to_string()));
        }
        
        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(&signature_bytes);
        
        let signature = Signature::from_bytes(&sig_array);
        
        // Verify signature
        let mut hash_transcript = transcript.clone();
        hash_transcript.append_message(b"seed_commit", &seed_commit);
        
        let mut challenge_bytes = [0u8; 64];
        hash_transcript.challenge_bytes(b"challenge", &mut challenge_bytes);
        
        self.verifying_key.verify(&challenge_bytes, &signature)
            .map_err(|_| VfError::InvalidProof("Signature verification failed".to_string()))?;
        
        Ok(true)
    }
}

// Thread-safe: VrfEngine can be shared across threads
unsafe impl Send for VrfEngine {}
unsafe impl Sync for VrfEngine {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vrf_engine_creation() {
        let engine = VrfEngine::new();
        assert!(!engine.node_pubkey().is_empty());
    }

    #[test]
    fn test_deterministic_generation() {
        let seed = [1u8; 32];
        let engine1 = VrfEngine::from_seed(seed);
        let engine2 = VrfEngine::from_seed(seed);
        
        assert_eq!(engine1.node_pubkey(), engine2.node_pubkey());
    }

    #[test]
    fn test_coinflip_processing() {
        let engine = VrfEngine::new();
        let req = CoinflipRequest {
            user_seed: "test_seed".to_string(),
            timestamp: 1234567890,
        };
        
        let result = engine.process_coinflip(&req);
        assert!(result.is_ok());
        
        let response = result.unwrap();
        assert!(!response.node_id.is_empty());
        assert!(!response.proof.seed_commitment.is_empty());
        assert!(!response.proof.vrf_output.is_empty());
        assert!(!response.proof.signature.is_empty());
    }

    #[test]
    fn test_proof_verification() {
        let engine = VrfEngine::new();
        let req = CoinflipRequest {
            user_seed: "test_seed".to_string(),
            timestamp: 1234567890,
        };
        
        let response = engine.process_coinflip(&req).unwrap();
        let verification = engine.verify_proof(&response.proof, &req);
        
        assert!(verification.is_ok());
        assert!(verification.unwrap());
    }

    #[test]
    fn test_invalid_proof_fails() {
        let engine = VrfEngine::new();
        let req = CoinflipRequest {
            user_seed: "test_seed".to_string(),
            timestamp: 1234567890,
        };
        
        let mut response = engine.process_coinflip(&req).unwrap();
        
        // Tamper with proof
        response.proof.signature = "invalid_signature".to_string();
        
        let verification = engine.verify_proof(&response.proof, &req);
        assert!(verification.is_err());
    }
}
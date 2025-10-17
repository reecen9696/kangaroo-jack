# VF Node - Verifiable Fair Coinflip Service

A Rust-based HTTP service that provides verifiable fair coinflip outcomes using VRF (Verifiable Random Function).

## Features

- **Instant Results**: Sub-100ms response times for coinflip bets
- **Cryptographically Fair**: Uses VRF with Merlin transcripts for provable randomness
- **Verifiable**: All outcomes include cryptographic proofs
- **Stateless**: No database required - pure compute service
- **HTTP API**: Simple JSON REST interface

## Quick Start

### 1. Run the server

```bash
cargo run
```

The server will start on `http://localhost:3000`

### 2. Test with the Python client

```bash
python3 test_client.py
```

### 3. Manual API test

```bash
curl -X POST http://localhost:3000/coinflip \
  -H "Content-Type: application/json" \
  -d '{
    "bet_id": "123e4567-e89b-12d3-a456-426614174000",
    "wager_lamports": 1000000,
    "token_mint": "SOL",
    "player_pubkey": "11111111111111111111111111111112",
    "client_seed": "deadbeef",
    "wallet_sig": "test_signature",
    "ts_ms": "2025-10-18T12:00:00Z"
  }'
```

## API Endpoints

### POST /coinflip

Process a coinflip bet.

**Request:**

```json
{
  "bet_id": "uuid",
  "wager_lamports": 1000000,
  "token_mint": "SOL",
  "player_pubkey": "base58_pubkey",
  "client_seed": "optional_hex_string",
  "wallet_sig": "base64_signature",
  "ts_ms": "2025-10-18T12:00:00Z"
}
```

**Response:**

```json
{
  "bet_id": "uuid",
  "heads": true,
  "win": true,
  "payout_lamports": 2000000,
  "proof": {
    "seed_commit": "hex_commitment",
    "random_value": "hex_random_output",
    "vrf_proof": "hex_vrf_proof",
    "node_signature": "base64_signature",
    "node_pubkey": "base64_pubkey"
  },
  "processed_at": "2025-10-18T12:00:01Z"
}
```

### GET /health

Health check endpoint.

### GET /info

Get node information including public key.

## Game Logic (MVP)

- Player always bets on "heads"
- Coin result determined by `random_value % 2 == 0`
- Win = 2x payout, Lose = 0 payout

## Cryptographic Design

1. **Transcript**: Built using Merlin with bet details + optional client seed
2. **VRF**: Challenge derived from transcript, used with Curve25519
3. **Signature**: Ed25519 signature over result digest
4. **Verification**: All proofs can be independently verified

## Development

### Run tests

```bash
cargo test
```

### Check the build

```bash
cargo check
```

### Environment variables

- `PORT`: Server port (default: 3000)
- `RUST_LOG`: Log level (default: info)

## Architecture

This is an MVP implementation of the larger VF Node design described in the attached document. Future enhancements:

- Multi-node consensus (M-of-N signatures)
- Batch settlement to Solana
- Additional games (dice, roulette)
- Wallet signature verification
- Rate limiting and abuse prevention

## License

MIT
# vfnode

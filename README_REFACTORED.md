# 🏗️ Production-Ready Rust Architecture Template v0.3.0

[![CI](https://github.com/Berektassuly/testable-rust-architecture-template/actions/workflows/ci.yml/badge.svg)](https://github.com/Berektassuly/testable-rust-architecture-template/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Production-ready Rust template demonstrating testable architecture through trait-based abstraction and dependency injection.

## 🆕 What's New in v0.3.0

This release implements all 7 production-readiness improvements:

### 1. ✅ SQLx Migration System
- Migrations in `migrations/20240101000000_create_items_table.sql`
- Proper indexes on frequently queried fields
- Run with `sqlx migrate run`

### 2. ✅ Cursor-Based Pagination
- `GET /items?limit=20&cursor=item_abc123`
- Stable pagination using `(created_at, id)` composite cursor
- Returns `next_cursor` and `has_more` for easy iteration

### 3. ✅ Rate Limiting (using `governor` crate)
- Separate limits for `/items` (10 RPS) and `/health` (100 RPS)
- Configurable via `RATE_LIMIT_RPS` and `RATE_LIMIT_BURST`
- Returns `X-RateLimit-*` headers and 429 responses

### 4. ✅ Real Solana Integration
- JSON-RPC client with retry logic
- Transaction signing via `ed25519-dalek`
- Feature flag: `real-blockchain` (disabled by default)

### 5. ✅ Graceful Degradation with Retry Queue
- Items saved with `pending_submission` status when blockchain unavailable
- Background worker for automatic retries
- Exponential backoff: 1s → 2s → 4s → ... → 5min max
- Manual retry via `POST /items/{id}/retry`

### 6. ✅ Database Integration Tests
- Uses `testcontainers` with PostgreSQL 16
- Tests CRUD, pagination, blockchain status updates
- Run with `cargo test --test database_integration`

### 7. ✅ OpenAPI Documentation
- Full API documentation at `/swagger-ui`
- OpenAPI 3.0 spec at `/api-docs/openapi.json`
- All types annotated with `#[derive(ToSchema)]`

## 📁 Project Structure

```
src/
├── api/
│   ├── handlers.rs     # Request handlers with OpenAPI annotations
│   ├── router.rs       # Routes, middleware, rate limiting
│   └── mod.rs
├── app/
│   ├── service.rs      # Business logic with graceful degradation
│   ├── state.rs        # Shared application state
│   ├── worker.rs       # Background retry worker
│   └── mod.rs
├── domain/
│   ├── error.rs        # Error types
│   ├── traits.rs       # Database & Blockchain traits
│   ├── types.rs        # Item, Pagination, BlockchainStatus
│   └── mod.rs
├── infra/
│   ├── blockchain/
│   │   └── solana.rs   # Solana RPC client
│   ├── database/
│   │   └── postgres.rs # PostgreSQL with pagination & status updates
│   └── mod.rs
└── test_utils/
    └── mocks.rs        # Mock implementations
migrations/
└── 20240101000000_create_items_table.sql
tests/
├── integration_test.rs
└── database_integration.rs
```

## 🚀 Quick Start

### Prerequisites
- Rust 1.85+
- PostgreSQL 14+
- Docker (for integration tests)

### Setup

```bash
# Clone
git clone https://github.com/Berektassuly/testable-rust-architecture-template.git
cd testable-rust-architecture-template

# Configure
cp .env.example .env
# Edit .env with your database credentials

# Install sqlx-cli
cargo install sqlx-cli --no-default-features --features postgres

# Run migrations
sqlx migrate run

# Start server
cargo run
```

## 📡 API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/items` | Create new item |
| `GET` | `/items` | List items (paginated) |
| `GET` | `/items/{id}` | Get single item |
| `POST` | `/items/{id}/retry` | Retry blockchain submission |
| `GET` | `/health` | Detailed health check |
| `GET` | `/health/live` | Kubernetes liveness probe |
| `GET` | `/health/ready` | Kubernetes readiness probe |
| `GET` | `/swagger-ui` | Interactive API documentation |
| `GET` | `/api-docs/openapi.json` | OpenAPI 3.0 specification |

### Example: List Items with Pagination

```bash
# First page
curl "http://localhost:3000/items?limit=10"

# Next page using cursor
curl "http://localhost:3000/items?limit=10&cursor=item_abc123"
```

Response:
```json
{
  "items": [...],
  "next_cursor": "item_xyz789",
  "has_more": true
}
```

## ⚙️ Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | *required* | PostgreSQL connection |
| `SOLANA_RPC_URL` | devnet | Blockchain RPC |
| `ISSUER_PRIVATE_KEY` | *generated* | Ed25519 key (base58) |
| `HOST` | `0.0.0.0` | Bind address |
| `PORT` | `3000` | Server port |
| `ENABLE_RATE_LIMITING` | `false` | Enable rate limiting |
| `RATE_LIMIT_RPS` | `10` | Requests per second |
| `RATE_LIMIT_BURST` | `20` | Burst allowance |
| `ENABLE_BACKGROUND_WORKER` | `true` | Enable retry worker |
| `RUST_LOG` | `info` | Log level |

## 🧪 Testing

```bash
# Unit tests
cargo test --lib

# Integration tests (requires PostgreSQL)
cargo test --test integration_test

# Database tests (requires Docker)
cargo test --test database_integration -- --test-threads=1

# All tests
cargo test --all-features
```

## 🔒 Blockchain Status Flow

```
[Pending] → [PendingSubmission] → [Submitted] → [Confirmed]
              ↓                                    
           [Failed] (after 10 retries)
```

## 📊 Observability

- Structured logging via `tracing`
- Health checks for Kubernetes deployments
- Rate limit headers for client guidance
- Background worker metrics in logs

## 📄 License

[MIT](LICENSE) © [Mukhammedali Berektassuly](https://berektassuly.com)

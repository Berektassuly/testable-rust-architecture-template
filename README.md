# 🏗️ Testable Rust Architecture Template

[![CI](https://github.com/Berektassuly/testable-rust-architecture-template/actions/workflows/ci.yml/badge.svg)](https://github.com/Berektassuly/testable-rust-architecture-template/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Production-ready Rust template demonstrating testable architecture through trait-based abstraction and dependency injection.

## ✨ Features

- **🧱 Clean Architecture** — Layered design with clear separation of concerns
- **💉 Dependency Injection** — Trait-based abstractions for testability
- **🔒 Security** — Input validation, secret management
- **⚡ Performance** — Connection pooling, async throughout
- **🧪 Testing** — Unit tests, integration tests, mock utilities
- **📊 Observability** — Structured logging, health checks
- **🚀 Production Ready** — Graceful shutdown, proper error handling

## 📁 Project Structure

```
src/
├── api/                    # HTTP layer
│   ├── handlers.rs         # Request handlers
│   └── router.rs           # Routes & middleware
├── app/                    # Application layer
│   ├── service.rs          # Business logic
│   └── state.rs            # Shared state
├── domain/                 # Domain layer
│   ├── error.rs            # Error types
│   ├── traits.rs           # Contracts
│   └── types.rs            # Models + validation
├── infra/                  # Infrastructure
│   ├── database/           # PostgreSQL
│   └── blockchain/         # RPC client
└── test_utils/             # Mocks
```

## 🏛️ Architecture

```
┌─────────────────────────────────────────┐
│              API Layer                  │
│   HTTP handlers, routing, validation    │
├─────────────────────────────────────────┤
│           Application Layer             │
│    Business logic, orchestration        │
├─────────────────────────────────────────┤
│             Domain Layer                │
│     Traits, types, errors (pure Rust)   │
├─────────────────────────────────────────┤
│          Infrastructure Layer           │
│   Database, blockchain, external APIs   │
└─────────────────────────────────────────┘
```

## 🚀 Quick Start

### Prerequisites

- Rust 1.85+
- PostgreSQL 14+

### Setup

```bash
# Clone
git clone https://github.com/Berektassuly/testable-rust-architecture-template.git
cd testable-rust-architecture-template

# Configure
cp .env.example .env
# Edit .env with your database credentials

# Create database
createdb app_dev

# Run
cargo run
```

### Test the API

```bash
# Health check
curl http://localhost:3000/health

# Create item
curl -X POST http://localhost:3000/items \
  -H "Content-Type: application/json" \
  -d '{"name": "My Item", "content": "Hello World"}'
```

## 🧪 Testing

```bash
# Run all tests
cargo test

# With coverage
cargo install cargo-tarpaulin
cargo tarpaulin --out Html
```

### Testing with Mocks

```rust
use testable_rust_architecture_template::test_utils::{MockDatabaseClient, MockBlockchainClient};

#[tokio::test]
async fn test_with_mocks() {
    let db = Arc::new(MockDatabaseClient::new());
    let blockchain = Arc::new(MockBlockchainClient::new());
    let state = Arc::new(AppState::new(db, blockchain));
    
    // Test your logic...
}
```

## 📡 API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/items` | Create new item |
| `GET` | `/health` | Detailed health check |
| `GET` | `/health/live` | Liveness probe (k8s) |
| `GET` | `/health/ready` | Readiness probe (k8s) |

### Create Item

```json
// Request
POST /items
{
  "name": "Item Name",
  "content": "Content here",
  "description": "Optional",
  "metadata": {
    "author": "Optional",
    "tags": ["tag1", "tag2"]
  }
}

// Response
{
  "id": "item_abc123",
  "hash": "hash_def456",
  "name": "Item Name",
  "created_at": "2025-01-15T10:30:00Z"
}
```

### Health Check

```json
// Response
{
  "status": "healthy",
  "database": "healthy",
  "blockchain": "healthy",
  "timestamp": "2025-01-15T10:30:00Z",
  "version": "0.2.0"
}
```

## 🔒 Security

### Input Validation

```rust
#[derive(Validate)]
pub struct CreateItemRequest {
    #[validate(length(min = 1, max = 255))]
    pub name: String,
    
    #[validate(length(max = 1_048_576))]  // 1MB
    pub content: String,
}
```

### Secret Management

```rust
use secrecy::{SecretString, ExposeSecret};

let private_key: SecretString = SecretString::from(key);
// Never accidentally logged
```

## ⚙️ Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | *required* | PostgreSQL connection |
| `SOLANA_RPC_URL` | devnet | Blockchain RPC |
| `ISSUER_PRIVATE_KEY` | *generated* | Ed25519 key (base58) |
| `HOST` | `0.0.0.0` | Bind address |
| `PORT` | `3000` | Server port |
| `RUST_LOG` | `info` | Log level |

## 📊 Observability

### Structured Logging

```rust
#[instrument(skip(self), fields(item_name = %request.name))]
pub async fn create_item(&self, request: &CreateItemRequest) {
    info!("Creating item");
}
```

### Log Configuration

```bash
# Development
RUST_LOG=debug,tower_http=trace

# Production
RUST_LOG=info,sqlx=warn
```

## 🐳 Docker

```dockerfile
FROM rust:1.85-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/testable-rust-architecture-template /usr/local/bin/
CMD ["testable-rust-architecture-template"]
```

## 📄 License

MIT
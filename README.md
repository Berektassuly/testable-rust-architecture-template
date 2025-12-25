# 🏗️ Testable Rust Architecture Template

A production-ready Rust template demonstrating testable architecture through trait-based abstraction and dependency injection.

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## ✨ Features

- **🧱 Clean Architecture** - Layered design with clear separation of concerns
- **💉 Dependency Injection** - Trait-based abstractions for testability
- **🔒 Security First** - Secret management, input validation, rate limiting
- **⚡ High Performance** - Connection pooling, async/await throughout
- **🧪 Comprehensive Testing** - Unit tests, integration tests, mock utilities
- **📊 Observability** - Structured logging with tracing, health checks
- **🚀 Production Ready** - Graceful shutdown, proper error handling

## 📁 Project Structure

```
src/
├── api/                    # HTTP layer
│   ├── handlers.rs         # Request handlers
│   ├── router.rs           # Route configuration & middleware
│   └── mod.rs
├── app/                    # Application layer
│   ├── service.rs          # Business logic orchestration
│   ├── state.rs            # Shared application state
│   └── mod.rs
├── domain/                 # Domain layer (no dependencies)
│   ├── error.rs            # Hierarchical error types
│   ├── traits.rs           # Contracts for external systems
│   ├── types.rs            # Domain models with validation
│   └── mod.rs
├── infra/                  # Infrastructure layer
│   ├── database/
│   │   ├── postgres.rs     # PostgreSQL implementation
│   │   └── mod.rs
│   ├── blockchain/
│   │   ├── solana.rs       # Blockchain RPC client
│   │   └── mod.rs
│   └── mod.rs
├── test_utils/             # Shared test utilities
│   ├── mocks.rs            # Mock implementations
│   └── mod.rs
├── lib.rs                  # Library entry point
└── main.rs                 # Application entry point

tests/
└── integration_test.rs     # Integration tests
```

## 🏛️ Architecture

```
┌─────────────────────────────────────────────┐
│                  API Layer                   │
│    HTTP handlers, routing, validation        │
├─────────────────────────────────────────────┤
│              Application Layer               │
│     Business logic, service orchestration    │
├─────────────────────────────────────────────┤
│                Domain Layer                  │
│      Traits, types, errors (pure Rust)       │
├─────────────────────────────────────────────┤
│             Infrastructure Layer             │
│   Database adapters, blockchain clients      │
└─────────────────────────────────────────────┘
```

### Key Design Principles

1. **Dependency Inversion** - High-level modules don't depend on low-level modules
2. **Interface Segregation** - Small, focused traits (`DatabaseClient`, `BlockchainClient`)
3. **Single Responsibility** - Each module has one reason to change
4. **Testability** - All external dependencies are mockable

## 🚀 Quick Start

### Prerequisites

- Rust 1.75 or later
- PostgreSQL 14+
- (Optional) Solana CLI for key generation

### Setup

1. **Clone the repository**
   ```bash
   git clone https://github.com/Berektassuly/testable-rust-architecture-template.git
   cd testable-rust-architecture-template
   ```

2. **Configure environment**
   ```bash
   cp .env.example .env
   # Edit .env with your database credentials
   ```

3. **Create database**
   ```bash
   createdb app_dev
   ```

4. **Run the application**
   ```bash
   cargo run
   ```

5. **Test the API**
   ```bash
   # Health check
   curl http://localhost:3000/health

   # Create an item
   curl -X POST http://localhost:3000/items \
     -H "Content-Type: application/json" \
     -d '{"name": "My Item", "content": "Hello World"}'
   ```

## 🧪 Testing

### Run all tests

```bash
cargo test
```

### Run with coverage

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --out Html
```

### Test structure

- **Unit tests** - In each module (`#[cfg(test)]`)
- **Integration tests** - In `tests/` directory
- **Mock utilities** - In `src/test_utils/`

### Example: Testing with mocks

```rust
use testable_rust_architecture_template::test_utils::{
    MockDatabaseClient, MockBlockchainClient
};

#[tokio::test]
async fn test_service_with_mocks() {
    let db = Arc::new(MockDatabaseClient::new());
    let blockchain = Arc::new(MockBlockchainClient::new());
    let service = AppService::new(db, blockchain);

    let result = service.create_and_submit_item(&request).await;
    assert!(result.is_ok());
}
```

## 📡 API Endpoints

| Method | Path           | Description           |
|--------|----------------|-----------------------|
| POST   | `/items`       | Create a new item     |
| GET    | `/health`      | Detailed health check |
| GET    | `/health/live` | Liveness probe (k8s)  |
| GET    | `/health/ready`| Readiness probe (k8s) |

### Create Item Request

```json
{
  "name": "Item Name",
  "content": "Item content (required)",
  "description": "Optional description",
  "metadata": {
    "author": "Optional author",
    "version": "1.0.0",
    "tags": ["tag1", "tag2"],
    "custom_fields": {
      "key": "value"
    }
  }
}
```

### Health Check Response

```json
{
  "status": "healthy",
  "database": "healthy",
  "blockchain": "healthy",
  "timestamp": "2024-01-15T10:30:00Z",
  "version": "0.2.0"
}
```

## 🔒 Security Features

### Input Validation

All request payloads are validated using the `validator` crate:

```rust
#[derive(Validate)]
pub struct CreateItemRequest {
    #[validate(length(min = 1, max = 255))]
    pub name: String,

    #[validate(length(max = 1_048_576))]  // 1MB max
    pub content: String,
}
```

### Secret Management

Sensitive data is protected using the `secrecy` crate:

```rust
use secrecy::{Secret, ExposeSecret};

let private_key: Secret<String> = Secret::new(key_string);
// Key is never accidentally logged
```

### Rate Limiting

Enable rate limiting in production:

```bash
ENABLE_RATE_LIMITING=true
```

Configured for 10 requests/second with burst of 50.

## 📊 Observability

### Structured Logging

Uses `tracing` for structured, contextual logging:

```rust
#[instrument(skip(self), fields(item_name = %request.name))]
pub async fn create_and_submit_item(&self, request: &CreateItemRequest) {
    info!("Creating new item");
    // ...
}
```

### Log Levels

Configure via `RUST_LOG` environment variable:

```bash
# Development
RUST_LOG=debug,tower_http=trace

# Production
RUST_LOG=info,sqlx=warn
```

## ⚙️ Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | Required | PostgreSQL connection string |
| `SOLANA_RPC_URL` | devnet | Blockchain RPC endpoint |
| `ISSUER_PRIVATE_KEY` | Generated | Ed25519 signing key |
| `HOST` | `0.0.0.0` | Server bind address |
| `PORT` | `3000` | Server port |
| `ENABLE_RATE_LIMITING` | `false` | Enable rate limiting |
| `RUST_LOG` | `info` | Log level filter |

## 🐳 Docker Support

```dockerfile
FROM rust:1.75-slim as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/testable-rust-architecture-template /usr/local/bin/
CMD ["testable-rust-architecture-template"]
```

## 🤝 Contributing

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Run tests (`cargo test`)
4. Run clippy (`cargo clippy -- -D warnings`)
5. Commit your changes (`git commit -m 'Add amazing feature'`)
6. Push to the branch (`git push origin feature/amazing-feature`)
7. Open a Pull Request

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [SQLx](https://github.com/launchbadge/sqlx) - Async database driver
- [Tokio](https://tokio.rs/) - Async runtime
- [Tracing](https://tracing.rs/) - Structured logging

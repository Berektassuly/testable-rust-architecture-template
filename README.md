# Rust Solana Transactional Outbox Service

![Build: Passing](https://img.shields.io/badge/build-passing-brightgreen)
![License: MIT](https://img.shields.io/badge/license-MIT-blue)
![Rust: 1.75+](https://img.shields.io/badge/rust-1.75%2B-orange)

A reliable, production-grade bridge between **PostgreSQL** and the **Solana blockchain** using the **Transactional Outbox** pattern. Built with Clean Architecture principles in Rust, the service guarantees that every domain event is eventually delivered to the blockchain with exactly-once semantics, even in the face of network failures and process crashes.

---

## Table of Contents

- [Architectural Overview](#architectural-overview)
- [Key Design Patterns](#key-design-patterns-the-why)
- [Configuration](#configuration)
- [Getting Started](#getting-started)
- [Testing](#testing)
- [API Documentation](#api-documentation)
- [API Endpoints](#api-endpoints)

---

## Architectural Overview

The application follows a strict **Clean Architecture** (Hexagonal / Ports-and-Adapters) layout. Each layer has a single responsibility and communicates only through trait-defined boundaries, making the entire system unit-testable without databases or network access.

```
src/
  api/          -- HTTP handlers, routing, middleware (Axum)
  app/          -- Application services, business logic orchestration
  domain/       -- Pure Rust: entities, traits (ports), errors
  infra/        -- Infrastructure adapters: Postgres, Solana RPC
  test_utils/   -- Mock implementations for all traits
```

| Layer            | Responsibility                                           | Key Files                         |
|------------------|----------------------------------------------------------|-----------------------------------|
| **API**          | HTTP routing, request validation, OpenAPI docs           | `handlers.rs`, `router.rs`, `middleware.rs` |
| **Application**  | Use-case orchestration, retry logic, health checks       | `service.rs`, `worker.rs`         |
| **Domain**       | Entity definitions, repository traits, error types       | `models.rs`, `traits.rs`          |
| **Infrastructure** | PostgreSQL repository, Solana RPC client, metrics      | `postgres.rs`, `blockchain.rs`    |

### System Data Flow

```mermaid
graph TD
    A["External Request"] --> B["API Layer (Axum)"]
    B --> C["App Service (Business Logic)"]
    C --> D["Domain Layer (Entities / Traits)"]
    C --> E["Infrastructure (Postgres Repository)"]
    E --> F["Atomic Insert: Item + Outbox Entry"]

    G["Background Worker"] --> H["Infrastructure (Postgres Outbox)"]
    H --> I["Claim Pending (FOR UPDATE SKIP LOCKED)"]
    G --> J["Infrastructure (Solana RPC)"]
    J --> K["Submit Transaction"]
    K -->|Success| L["Mark Outbox Complete + Update Item"]
    K -->|Failure| M["Sticky Blockhash Retry with Backoff"]
```

---

## Key Design Patterns (The "Why")

### Transactional Outbox

The core reliability guarantee lives in `infra/database/postgres.rs`, inside the `create_item` method. When a new item is created, both the `items` row and its corresponding `solana_outbox` row are inserted within a **single database transaction**:

```rust
let mut tx = self.pool.begin().await?;

// 1. Insert the item
sqlx::query("INSERT INTO items ...").execute(&mut *tx).await?;

// 2. Insert the outbox entry (blockchain intent)
sqlx::query("INSERT INTO solana_outbox ...").execute(&mut *tx).await?;

// 3. Commit atomically
tx.commit().await?;
```

This guarantees **atomicity**: an item is never persisted without a corresponding blockchain submission intent. If the process crashes after commit, the outbox entry survives and will be picked up by the background worker. If the transaction rolls back, neither the item nor the outbox entry exists -- no orphaned state.

### Sticky Blockhash Strategy (Double-Spend Prevention)

The `process_outbox_entry` method in `app/service.rs` implements a **sticky blockhash** mechanism to prevent double-spend on retries. The problem: if a Solana transaction is submitted but the response is lost (timeout, network error), the transaction may have landed on-chain. Retrying with a *new* blockhash would produce a *different* signature, potentially spending funds twice.

The solution:

1. On the **first attempt**, the blockchain client fetches a fresh `recent_blockhash` and returns it alongside the signature.
2. If submission **fails with a recoverable error** (timeout, network error, or submission failure with a known blockhash), the `blockhash_used` is **persisted** to the `attempt_blockhash` column in `solana_outbox`.
3. On **retry**, the persisted blockhash is supplied to `submit_transaction`. The transaction is reconstructed with the **same blockhash**, producing an **identical signature**. If the original transaction already landed, the retry is a harmless duplicate.
4. The blockhash is only **cleared** on `BlockhashExpired`, which confirms the original transaction could not have been processed.

```
First Attempt:  fetch_blockhash() -> sign(data, blockhash_A) -> submit -> TIMEOUT
                persist attempt_blockhash = blockhash_A

Retry:          reuse blockhash_A  -> sign(data, blockhash_A) -> submit -> OK
                identical signature = idempotent
```

### Concurrency Control (Horizontal Worker Scaling)

The SQL queries in both `claim_pending_solana_outbox` and `get_pending_blockchain_items` use PostgreSQL's `FOR UPDATE SKIP LOCKED` clause:

```sql
WITH candidate AS (
    SELECT id
    FROM solana_outbox
    WHERE status = 'pending'
      AND (next_retry_at IS NULL OR next_retry_at <= $1)
    ORDER BY created_at ASC
    LIMIT $2
    FOR UPDATE SKIP LOCKED
)
UPDATE solana_outbox o
SET status = 'processing', updated_at = NOW()
FROM candidate
WHERE o.id = candidate.id
RETURNING ...
```

This enables **safe horizontal scaling**: multiple worker instances can poll the outbox concurrently without processing the same entry. Each worker atomically claims a batch of rows; any rows already locked by another worker are silently skipped. No external coordination (Redis, ZooKeeper) is required.

---

## Configuration

All configuration is loaded from environment variables. The application reads a `.env` file on startup via `dotenvy`.

### Environment Variables

| Variable                   | Required | Default                            | Description                                                    |
|----------------------------|----------|------------------------------------|----------------------------------------------------------------|
| `DATABASE_URL`             | Yes      | --                                 | PostgreSQL connection string                                   |
| `API_AUTH_KEY`             | Yes      | --                                 | API key for authenticating `POST` requests (`x-api-key` header)|
| `SOLANA_RPC_URL`           | No       | `https://api.devnet.solana.com`    | Solana JSON-RPC endpoint                                       |
| `SIGNER_TYPE`              | No       | `LOCAL`                            | Transaction signer: `LOCAL` or `KMS`                           |
| `ISSUER_PRIVATE_KEY`       | No       | Ephemeral keypair generated        | Base58-encoded Ed25519 private key (when `SIGNER_TYPE=LOCAL`)  |
| `KMS_KEY_ID`               | Cond.    | --                                 | AWS KMS key ID (required when `SIGNER_TYPE=KMS`)               |
| `HOST`                     | No       | `0.0.0.0`                          | Server bind address                                            |
| `PORT`                     | No       | `3000`                             | Server listen port                                             |
| `ENABLE_RATE_LIMITING`     | No       | `false`                            | Enable request rate limiting                                   |
| `RATE_LIMIT_RPS`           | No       | `10`                               | Rate limit: requests per second                                |
| `RATE_LIMIT_BURST`         | No       | `20`                               | Rate limit: burst capacity                                     |
| `ENABLE_BACKGROUND_WORKER` | No       | `true`                             | Enable the outbox background worker                            |
| `RUST_LOG`                 | No       | `info,tower_http=debug,sqlx=warn`  | Tracing filter directive                                       |

### PostgreSQL Pool Configuration (Compile-Time Defaults)

| Parameter         | Default |
|-------------------|---------|
| `max_connections`  | 10      |
| `min_connections`  | 2       |
| `acquire_timeout`  | 3s      |
| `idle_timeout`     | 600s    |
| `max_lifetime`     | 1800s   |

---

## Getting Started

### Prerequisites

- **Rust** toolchain (1.75+ recommended) -- install via [rustup](https://rustup.rs)
- **Docker** (for PostgreSQL)
- **Solana CLI** (optional, for key management)

### Local Setup

**1. Start PostgreSQL**

Using Docker Compose (recommended):

```bash
docker compose up -d
```

Or directly with Docker:

```bash
docker run --name pg \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=app_dev \
  -p 5432:5432 \
  -d postgres:16-alpine
```

**2. Configure Environment**

Copy the example environment file and edit as needed:

```bash
cp .env.example .env
```

At minimum, set the required variables:

```env
DATABASE_URL=postgres://postgres:postgres@localhost:5432/app_dev
API_AUTH_KEY=your-secret-api-key-here
SOLANA_RPC_URL=https://api.devnet.solana.com
ISSUER_PRIVATE_KEY=YOUR_BASE58_ENCODED_PRIVATE_KEY_HERE
```

**3. Run the Application**

Migrations are applied automatically on startup by `PostgresClient::run_migrations()`.

```bash
cargo run
```

The server will start on `http://0.0.0.0:3000` by default. You should see output similar to:

```
INFO  Server starting on http://0.0.0.0:3000
INFO  Swagger UI available at http://0.0.0.0:3000/swagger-ui
INFO  OpenAPI spec at http://0.0.0.0:3000/api-docs/openapi.json
```

---

## Testing

### Strategy

The codebase uses **trait-based dependency injection** to achieve full testability without external services. The `test_utils` module (enabled via the `test-utils` feature flag) provides:

- **`MockProvider`**: An in-memory implementation of both `ItemRepository` and `OutboxRepository`. Stores items and outbox entries in `Arc<RwLock<HashMap<...>>>` for thread-safe concurrent test access.
- **`MockBlockchainClient`**: A configurable mock that can simulate successful submissions or controlled failures (via `MockBlockchainClient::failing("error message")`).
- **`mock_repos()`**: A convenience function that returns `(Arc<dyn ItemRepository>, Arc<dyn OutboxRepository>)` backed by the same `MockProvider` instance.

This design means every layer -- handlers, services, and error mapping -- can be tested in isolation with sub-millisecond execution.

### Running Tests

```bash
cargo test
```

For integration tests that use testcontainers (requires Docker):

```bash
cargo test --test '*'
```

---

## API Documentation

The application serves interactive **OpenAPI / Swagger UI** documentation, generated at compile time via `utoipa`.

| Resource      | URL                                          |
|---------------|----------------------------------------------|
| Swagger UI    | `http://localhost:3000/swagger-ui`           |
| OpenAPI JSON  | `http://localhost:3000/api-docs/openapi.json`|

---

## API Endpoints

All `POST` endpoints require the `x-api-key` header for authentication.

### Items

| Method | Path               | Auth | Description                                |
|--------|---------------------|------|--------------------------------------------|
| `POST` | `/items`            | Yes  | Create a new item and enqueue for blockchain submission |
| `GET`  | `/items`            | No   | List items with cursor-based pagination    |
| `GET`  | `/items/{id}`       | No   | Retrieve a single item by ID               |
| `POST` | `/items/{id}/retry` | Yes  | Retry blockchain submission for a failed item |

### Health

| Method | Path            | Auth | Description                                 |
|--------|-----------------|------|---------------------------------------------|
| `GET`  | `/health`       | No   | Detailed health check (database + blockchain) |
| `GET`  | `/health/live`  | No   | Kubernetes liveness probe                   |
| `GET`  | `/health/ready` | No   | Kubernetes readiness probe                  |

### Observability

| Resource             | URL                               | Description                      |
|----------------------|-----------------------------------|----------------------------------|
| Prometheus Metrics   | `http://localhost:3000/metrics`   | Prometheus-format metrics export |
| Swagger UI           | `http://localhost:3000/swagger-ui`| Interactive API documentation    |

---

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

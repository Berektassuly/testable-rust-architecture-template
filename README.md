# Testable Rust Architecture Template

[![CI](https://github.com/Berektassuly/testable-rust-architecture-template/actions/workflows/ci.yml/badge.svg)](https://github.com/Berektassuly/testable-rust-architecture-template/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A production-grade reference implementation for building scalable, resilient, and testable backend services in Rust. This template implements Clean Architecture principles with strict layer separation, dependency injection via traits, and robust integration with PostgreSQL and Solana.

## Overview

This project serves as a blueprint for high-assurance Rust applications. It solves common architectural challenges by decoupling business logic from infrastructure concerns, enabling independent testing of components and graceful degradation in distributed systems.

### Key Features

*   **Clean Architecture:** Strict separation of concerns into API, Application, Domain, and Infrastructure layers.
*   **Dependency Injection:** Logic relies on traits rather than concrete implementations, facilitating mocking and testing.
*   **Resilience & Reliability:**
    *   **Graceful Degradation:** Fallback mechanisms when external services (Blockchain) are unavailable.
    *   **Background Workers:** Asynchronous retry queues for eventual consistency.
    *   **Rate Limiting:** In-memory request throttling via `governor`.
*   **Observability:** Structured logging with `tracing` and auto-generated OpenAPI (Swagger) documentation.
*   **Database:** Compile-time checked SQL queries using `sqlx` and PostgreSQL.
*   **Blockchain Integration:** Solana RPC client with transaction signing and confirmation tracking.
*   **Testing Strategy:** Comprehensive suite including unit tests, mock-based testing, and containerized integration tests using `testcontainers`.

## Architecture

The application follows a unidirectional dependency flow. The Domain layer contains pure business rules and interfaces, while the Infrastructure layer implements those interfaces.

```text
src/
├── api/        # Presentation Layer
│               # - HTTP Handlers (Axum)
│               # - Route configuration
│               # - Rate limiting middleware
│               # - OpenAPI definition
│
├── app/        # Application Layer
│               # - Service orchestration
│               # - Background worker logic
│               # - Application state
│
├── domain/     # Domain Layer (Pure Rust)
│               # - Entities and DTOs
│               # - Interface definitions (Traits)
│               # - Domain Errors
│
└── infra/      # Infrastructure Layer
                # - PostgreSQL adapter (SQLx)
                # - Solana RPC client
                # - External service implementations
```

## Getting Started

### Prerequisites

*   **Rust:** Latest stable toolchain.
*   **Docker:** Required for running the local database and executing integration tests via `testcontainers`.
*   **PostgreSQL:** Version 16+ (if running locally without Docker).

### Configuration

1.  Clone the repository:
    ```bash
    git clone https://github.com/Berektassuly/testable-rust-architecture-template.git
    cd testable-rust-architecture-template
    ```

2.  Initialize configuration:
    ```bash
    cp .env.example .env
    ```

3.  Edit `.env` to set your specific configuration. For local development, the default values usually suffice.

### Database Setup

The easiest way to set up the database is using Docker Compose:

```bash
docker-compose up -d
```

Alternatively, if you want to manage migrations manually or use a local PostgreSQL instance:

1. Install the SQLx CLI tool:
   ```bash
   cargo install sqlx-cli --no-default-features --features postgres
   ```

2. Run migrations:
   ```bash
   # Ensure the database defined in DATABASE_URL exists or use:
   # sqlx database create
   sqlx migrate run --source ./migrations
   ```

### Running the Application

Start the server:

```bash
cargo run
```

By default, the server listens on `http://0.0.0.0:3000`.

## API Documentation

The application automatically generates an OpenAPI specification and hosts a Swagger UI.

*   **Swagger UI:** `http://localhost:3000/swagger-ui`
*   **OpenAPI Spec:** `http://localhost:3000/api-docs/openapi.json`

## Configuration Reference

The application is configured via environment variables.

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | PostgreSQL connection string | `postgres://postgres:postgres@localhost:5432/app_dev` |
| `SOLANA_RPC_URL` | Solana Cluster Endpoint | `https://api.devnet.solana.com` |
| `ISSUER_PRIVATE_KEY` | Base58 private key for signing | *(Generated ephemeral key if unset)* |
| `HOST` | Server bind address | `0.0.0.0` |
| `PORT` | Server bind port | `3000` |
| `ENABLE_RATE_LIMITING` | Toggle API rate limiting | `false` |
| `RATE_LIMIT_RPS` | Requests per second limit | `10` |
| `ENABLE_BACKGROUND_WORKER` | Enable the retry worker | `true` |
| `RUST_LOG` | Tracing log level | `info,tower_http=debug` |

## Build Features

This template uses Cargo features to manage external dependencies and build targets.

*   `default`: Uses a mock blockchain implementation for faster development.
*   `real-blockchain`: Enables actual Solana network interaction.
*   `test-utils`: Exposes mock clients and utilities for integration testing.

To build for production with real blockchain integration:
```bash
cargo build --release --features real-blockchain
```

## Testing

The project employs a multi-level testing strategy to ensure reliability without sacrificing development speed.

### Unit Tests
Fast, isolated tests using mocks for database and blockchain clients.

```bash
cargo test --lib
```

### Integration Tests
End-to-end API tests and database integration tests. These require Docker as they spin up ephemeral PostgreSQL containers.

```bash
# Run API integration tests
cargo test --test integration_test

# Run Database integration tests
cargo test --test database_integration
```

### Benchmarks
Performance benchmarks for critical domain logic (e.g., validation, hashing) using `criterion`.

```bash
cargo bench
```

### Security Audit
Check for vulnerabilities in the dependency tree.

```bash
cargo install cargo-audit
cargo audit
```

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
## 📄 License

[MIT](LICENSE) © [Mukhammedali Berektassuly](https://berektassuly.com)
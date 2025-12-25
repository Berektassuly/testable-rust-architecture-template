# Testable Rust Architecture Template

Production-ready Rust template with trait-based DI and testability.

## Quick Start

```bash
cp .env.example .env
# Edit .env with your database credentials
cargo run
```

## Endpoints

- `POST /items` - Create item
- `GET /health` - Health check
- `GET /health/live` - Liveness probe
- `GET /health/ready` - Readiness probe

## Test

```bash
cargo test
```

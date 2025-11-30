# testable-rust-architecture-template

> A production-ready Rust template demonstrating testable architecture through trait-based abstraction and dependency injection for services interacting with external systems.

This template is the reference implementation for the architectural principles described in the article **[Architecture as LEGO](https://berektassuly.com/architecture-as-lego-rust-testing)**. It provides a clean foundation for building real-world, scalable microservices in Rust.

## Core Concepts

This template is built around the following key principles:

- **Trait-based abstraction** over external dependencies.
- **Async trait pattern** for I/O-bound operations using `async-trait`.
- **Dynamic dispatch via `Arc<dyn Trait>`** for runtime flexibility.
- **In-memory mock implementations** for fast, isolated unit testing.
- **Dependency injection** through a shared application state.
- **Clean separation** of business logic from infrastructure concerns.
- **Design for replaceability,** not prediction.

## Project Structure

The project follows a Clean Architecture / Hexagonal approach, separating concerns into distinct modules:

```tree
.
├── src/
│   ├── domain/     # Core business logic, traits, and types. No external dependencies.
│   ├── app/        # Application services that orchestrate domain logic.
│   ├── infra/      # Concrete implementations of domain traits (DB, Blockchain clients).
│   ├── api/        # Web handlers and routing (Axum).
│   └── main.rs     # Composition root: wiring everything together.
└── ...
```

- **`domain/`** — Core traits (contracts) defining what external systems must do, plus shared domain types and errors.
- **`app/`** — Application services containing business logic that depend only on domain traits.
- **`infra/`** — Concrete implementations (adapters) for databases, blockchains, and external APIs.

## Getting Started

To run this service locally, follow these steps:

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/Berektassuly/testable-rust-architecture-template.git
    cd testable-rust-architecture-template
    ```

2.  **Configure environment:**
    ```bash
    cp .env.example .env
    # Now, edit the .env file with your configuration
    ```

3.  **Build the project:**
    ```bash
    cargo build
    ```

4.  **Run the server:**
    ```bash
    cargo run
    ```

## Running Tests

This architecture is designed for fast, reliable testing. To run all unit and integration tests, simply use:

```bash
cargo test
```

The unit tests for the application service run in milliseconds, as they use in-memory mocks and require no network or database connections.
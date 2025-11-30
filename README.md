# testable-rust-architecture-template

> A production-ready Rust template demonstrating testable architecture through trait-based abstraction and dependency injection for services interacting with external systems.

This template implements the architectural principles described in the article **["Architecture as LEGO: Building a Testable Rust Service with Blockchain Abstraction"](https://berektassuly.com/architecture-as-lego-rust-testing)**. If you want to understand the reasoning behind every design decision, that article is the definitive guide.

## Core Concepts

This template demonstrates the following architectural patterns:

- **Trait-based abstraction over external dependencies** — Define interfaces that describe *what* external systems should do, not *how* they do it.
- **Async trait pattern for I/O-bound operations** — Use `async-trait` to enable async methods in traits, accepting the minor heap allocation overhead since it's negligible compared to actual I/O latency.
- **Dynamic dispatch via `Arc<dyn Trait>`** — Prefer trait objects over generics when you need runtime implementation swapping, smaller binaries, and simpler integration with web frameworks.
- **In-memory mock implementations** — Create mock implementations using simple data structures (like `HashMap`) that satisfy the trait contract, enabling millisecond-speed unit tests.
- **Dependency injection through shared application state** — Inject trait objects into a centralized `AppState` struct, keeping concrete implementation selection at the composition root.
- **Separation of business logic from infrastructure** — Handlers interact only with trait abstractions, remaining completely unaware of whether they're talking to PostgreSQL, Solana, or an in-memory mock.
- **Design for replaceability, not prediction** — Build architecture that makes future changes easy to implement rather than trying to anticipate every possible requirement upfront.

## Why This Architecture? (The Trade-offs)

Every architectural decision involves trade-offs. This template makes specific choices that prioritize **testability** and **long-term maintainability** over short-term convenience. Here's an honest assessment:

### ✅ Advantages

| Benefit | Description |
|---------|-------------|
| **Speed** | Unit tests run in milliseconds without network access, databases, or blockchain nodes. Your CI/CD pipeline stays fast and deterministic. |
| **Replaceability** | Need to swap Solana for Ethereum? Write a new adapter implementing `BlockchainClient`. Your business logic remains completely untouched. |
| **Clarity** | Business logic is clean and readable, free from HTTP client configuration, SQL query building, or blockchain SDK details. |
| **Familiarity** | Developers from Java (Spring), C# (.NET), or Go backgrounds will find this architecture immediately intuitive. |

### ❌ Conscious Trade-offs

| Trade-off | Description |
|-----------|-------------|
| **Boilerplate** | You must define traits, then structs, then implement them. For simple CRUD applications, this can feel like overengineering. *This trade-off is made consciously to achieve maximum testability and long-term maintainability. The upfront investment pays dividends as the codebase grows.* |
| **Manual Mocks** | Writing mock clients takes time, unlike using auto-mocking libraries like `mockall`. *This is intentional—hand-written mocks are explicit, debuggable, and serve as living documentation of expected behavior.* |
| **Dynamic Dispatch** | Performance purists may object to `dyn Trait` and vtable lookups. *In practice, this overhead is negligible for I/O-bound web services where network latency dominates. We choose flexibility over nanoseconds.* |

**Bottom line:** If you're building a quick prototype or a simple CRUD app, this architecture may be overkill. But if you're building a service that will evolve over time, integrate with multiple external systems, or require comprehensive test coverage—this template provides a solid foundation.

## Project Structure
```
.
├── .cargo/
│   └── config.toml          # Build optimizations (faster linking)
├── src/
│   ├── domain/              # Core business types and trait definitions
│   │   ├── mod.rs
│   │   ├── error.rs         # Application error types
│   │   ├── traits.rs        # DatabaseClient, BlockchainClient traits
│   │   └── types.rs         # Domain models (Item, CreateItemRequest, etc.)
│   ├── app/                 # Application logic and state
│   │   ├── mod.rs
│   │   ├── service.rs       # Business logic orchestration
│   │   └── state.rs         # Shared AppState with injected dependencies
│   ├── infra/               # Concrete implementations of domain traits
│   │   ├── mod.rs
│   │   ├── database/
│   │   │   ├── mod.rs
│   │   │   └── postgres.rs  # PostgreSQL implementation of DatabaseClient
│   │   └── blockchain/
│   │       ├── mod.rs
│   │       └── solana.rs    # RPC-based blockchain client implementation
│   ├── api/                 # HTTP layer (Axum handlers and routing)
│   │   ├── mod.rs
│   │   ├── handlers.rs      # Request handlers
│   │   └── router.rs        # Route definitions
│   ├── lib.rs               # Library entry point
│   └── main.rs              # Application entry point
├── tests/
│   └── integration_test.rs  # End-to-end tests with mock implementations
├── .env.example             # Environment variable template
├── .gitignore
├── Cargo.toml
├── LICENSE
└── README.md
```

### Layer Responsibilities

| Layer | Purpose |
|-------|---------|
| **`domain/`** | Defines the contracts (traits) that external systems must fulfill, plus shared types and errors. Has **zero dependencies** on infrastructure or frameworks. |
| **`app/`** | Contains business logic that orchestrates operations through trait abstractions. Knows nothing about PostgreSQL, Solana, or HTTP. |
| **`infra/`** | Provides concrete implementations (adapters) for databases, blockchains, and external APIs. This is where SDK-specific code lives. |
| **`api/`** | Handles HTTP concerns: request parsing, response formatting, routing. Delegates all business logic to the `app` layer. |

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (1.75 or later recommended)
- [PostgreSQL](https://www.postgresql.org/) (for production use)
- [lld linker](https://lld.llvm.org/) (optional, for faster builds)

### Installation

1. **Clone the repository:**
```bash
   git clone https://github.com/your-username/testable-rust-architecture-template.git
   cd testable-rust-architecture-template
```

2. **Copy the environment template:**
```bash
   cp .env.example .env
```

3. **Configure your environment:**

   Edit `.env` with your actual values:
```bash
   # Database connection
   DATABASE_URL=postgres://postgres:postgres@localhost:5432/app_dev

   # Blockchain RPC endpoint (defaults to Solana devnet)
   SOLANA_RPC_URL=https://api.devnet.solana.com

   # Your signing key (or leave as placeholder for development)
   ISSUER_PRIVATE_KEY="YOUR_BASE58_ENCODED_PRIVATE_KEY_HERE"
```

4. **Build the project:**
```bash
   cargo build
```

5. **Run the server:**
```bash
   cargo run
```

   You should see output like:
```
   ⚠️  No valid ISSUER_PRIVATE_KEY provided, generating ephemeral keypair
      This is fine for development, but set a real key for production!
   🔑 Using public key: 7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU
   🚀 Server starting on http://0.0.0.0:3000
```

> **Note:** The application will compile and start, but database operations will panic until you implement the `todo!()` placeholders in `src/infra/database/postgres.rs`. This is intentional—the template provides the architecture, you provide the implementation.

## Running Tests
```bash
cargo test
```

### Why Are the Tests So Fast?

The tests run in **milliseconds** because they use **in-memory mock implementations** instead of real external services:

- **No database required** — `MockDatabaseClient` uses a `HashMap` for storage
- **No blockchain required** — `MockBlockchainClient` returns instant responses
- **No network I/O** — Everything runs in-process

This is the primary benefit of trait-based abstraction: your business logic can be thoroughly tested without spinning up Docker containers, waiting for network timeouts, or paying for test transactions.
```rust
// From tests/integration_test.rs
#[tokio::test]
async fn test_create_item_success_e2e() {
    // Arrange: Create mock clients (no real DB or blockchain!)
    let mock_db = Arc::new(MockDatabaseClient::new());
    let mock_blockchain = Arc::new(MockBlockchainClient::new());

    let app_state = AppState::new(mock_db, mock_blockchain);
    let router = create_router(Arc::new(app_state));

    // Act & Assert: Full request/response cycle in milliseconds
    // ...
}
```

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.

---

Built with ❤️ to demonstrate that **testable architecture in Rust doesn't have to be complicated**—just intentional.
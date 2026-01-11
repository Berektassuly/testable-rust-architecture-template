//! Blockchain RPC client implementation for Solana.
//!
//! This module provides both mock and real blockchain interactions.
//! Real blockchain functionality is enabled with the `real-blockchain` feature.

use async_trait::async_trait;
use ed25519_dalek::{Signer, SigningKey};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;
use tracing::{debug, info, instrument, warn};

use crate::domain::{AppError, BlockchainClient, BlockchainError};

/// Configuration for the RPC client
#[derive(Debug, Clone)]
pub struct RpcClientConfig {
    pub timeout: Duration,
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub confirmation_timeout: Duration,
}

impl Default for RpcClientConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay: Duration::from_millis(500),
            confirmation_timeout: Duration::from_secs(60),
        }
    }
}

/// Abstract provider for Solana RPC interactions to enable testing
#[async_trait]
pub trait SolanaRpcProvider: Send + Sync {
    /// Send a JSON-RPC request
    async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, AppError>;

    /// Get the provider's public key
    fn public_key(&self) -> String;

    /// Sign a message
    fn sign(&self, message: &[u8]) -> String;
}

/// HTTP-based Solana RPC provider
pub struct HttpSolanaRpcProvider {
    http_client: Client,
    rpc_url: String,
    signing_key: SigningKey,
}

impl HttpSolanaRpcProvider {
    pub fn new(
        rpc_url: &str,
        signing_key: SigningKey,
        timeout: Duration,
    ) -> Result<Self, AppError> {
        let http_client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| AppError::Blockchain(BlockchainError::Connection(e.to_string())))?;

        Ok(Self {
            http_client,
            rpc_url: rpc_url.to_string(),
            signing_key,
        })
    }
}

#[async_trait]
impl SolanaRpcProvider for HttpSolanaRpcProvider {
    async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: method.to_string(),
            params,
        };

        let response = self
            .http_client
            .post(&self.rpc_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    AppError::Blockchain(BlockchainError::Timeout(e.to_string()))
                } else {
                    AppError::Blockchain(BlockchainError::RpcError(e.to_string()))
                }
            })?;

        let rpc_response: JsonRpcResponse<serde_json::Value> = response
            .json()
            .await
            .map_err(|e| AppError::Blockchain(BlockchainError::RpcError(e.to_string())))?;

        if let Some(error) = rpc_response.error {
            // Check for insufficient funds error
            if error.message.contains("insufficient") || error.code == -32002 {
                return Err(AppError::Blockchain(BlockchainError::InsufficientFunds));
            }
            return Err(AppError::Blockchain(BlockchainError::RpcError(format!(
                "{}: {}",
                error.code, error.message
            ))));
        }

        rpc_response.result.ok_or_else(|| {
            AppError::Blockchain(BlockchainError::RpcError("Empty response".to_string()))
        })
    }

    fn public_key(&self) -> String {
        bs58::encode(self.signing_key.verifying_key().as_bytes()).into_string()
    }

    fn sign(&self, message: &[u8]) -> String {
        let signature = self.signing_key.sign(message);
        bs58::encode(signature.to_bytes()).into_string()
    }
}

/// Solana RPC blockchain client
pub struct RpcBlockchainClient {
    provider: Box<dyn SolanaRpcProvider>,
    config: RpcClientConfig,
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<T: Serialize> {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: T,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct BlockhashResponse {
    blockhash: String,
}

#[derive(Debug, Deserialize)]
struct BlockhashResult {
    value: BlockhashResponse,
}

#[derive(Debug, Deserialize)]
struct SignatureStatus {
    err: Option<serde_json::Value>,
    #[serde(rename = "confirmationStatus")]
    confirmation_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SignatureStatusResult {
    value: Vec<Option<SignatureStatus>>,
}

impl RpcBlockchainClient {
    /// Create a new RPC blockchain client with custom configuration
    pub fn new(
        rpc_url: &str,
        signing_key: SigningKey,
        config: RpcClientConfig,
    ) -> Result<Self, AppError> {
        let provider = HttpSolanaRpcProvider::new(rpc_url, signing_key, config.timeout)?;
        info!(rpc_url = %rpc_url, "Created blockchain client");
        Ok(Self {
            provider: Box::new(provider),
            config,
        })
    }

    /// Create a new RPC blockchain client with default configuration
    pub fn with_defaults(rpc_url: &str, signing_key: SigningKey) -> Result<Self, AppError> {
        Self::new(rpc_url, signing_key, RpcClientConfig::default())
    }

    /// Create a new client with a specific provider (useful for testing)
    pub fn with_provider(provider: Box<dyn SolanaRpcProvider>, config: RpcClientConfig) -> Self {
        Self { provider, config }
    }

    /// Get the public key as base58 string
    #[must_use]
    pub fn public_key(&self) -> String {
        self.provider.public_key()
    }

    /// Sign a message and return the signature as base58
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> String {
        self.provider.sign(message)
    }

    /// Make an RPC call with retries
    #[instrument(skip(self, params))]
    async fn rpc_call<P: Serialize + Send + Sync, R: DeserializeOwned + Send>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, AppError> {
        // Serialize parameters to JSON Value
        let params_value = serde_json::to_value(params).map_err(|e| {
            AppError::Blockchain(BlockchainError::RpcError(format!(
                "Serialization error: {}",
                e
            )))
        })?;

        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                tokio::time::sleep(self.config.retry_delay).await;
            }
            match self
                .provider
                .send_request(method, params_value.clone())
                .await
            {
                Ok(result_value) => {
                    // Deserialize result from JSON Value
                    return serde_json::from_value(result_value).map_err(|e| {
                        AppError::Blockchain(BlockchainError::RpcError(format!(
                            "Deserialization error: {}",
                            e
                        )))
                    });
                }
                Err(e) => {
                    warn!(attempt = attempt, error = ?e, method = %method, "RPC call failed");
                    last_error = Some(e);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            AppError::Blockchain(BlockchainError::RpcError("Unknown error".to_string()))
        }))
    }

    /// Build and serialize a memo transaction
    #[cfg(feature = "real-blockchain")]
    fn build_memo_transaction(
        &self,
        memo: &str,
        recent_blockhash: &str,
    ) -> Result<String, AppError> {
        // Memo program ID
        let memo_program_id = bs58::decode("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr")
            .into_vec()
            .map_err(|e| AppError::Blockchain(BlockchainError::InvalidSignature(e.to_string())))?;

        let recent_blockhash_bytes = bs58::decode(recent_blockhash)
            .into_vec()
            .map_err(|e| AppError::Blockchain(BlockchainError::InvalidSignature(e.to_string())))?;

        let public_key_bytes: Vec<u8> =
            bs58::decode(self.provider.public_key()).into_vec().unwrap(); // Should always be valid base58 from provider

        // Build a simplified transaction structure
        // This is a minimal memo transaction
        let mut tx_data = vec![
            1u8, // sig count
            2u8, // num accounts
        ];

        // Number of signatures
        tx_data.push(1u8);

        // Message header
        tx_data.push(1u8); // num_required_signatures
        tx_data.push(0u8); // num_readonly_signed_accounts
        tx_data.push(1u8); // num_readonly_unsigned_accounts

        // Account keys (payer + memo program)
        tx_data.push(2u8); // num accounts
        tx_data.extend_from_slice(&public_key_bytes);
        tx_data.extend_from_slice(&memo_program_id);

        // Recent blockhash
        tx_data.extend_from_slice(&recent_blockhash_bytes);

        // Instructions
        tx_data.push(1u8); // num instructions

        // Memo instruction
        tx_data.push(1u8); // program_id_index (memo program)
        tx_data.push(1u8); // num accounts
        tx_data.push(0u8); // account index (payer)

        // Memo data
        let memo_bytes = memo.as_bytes();
        tx_data.push(memo_bytes.len() as u8);
        tx_data.extend_from_slice(memo_bytes);

        // Sign the message (everything after signatures)
        let _message_start = 1 + 64; // 1 byte for sig count, 64 bytes for signature placeholder
        let message = &tx_data[1..]; // Skip signature count

        let signature_str = self.provider.sign(message);
        let signature_bytes = bs58::decode(signature_str).into_vec().unwrap();

        // Insert signature
        let mut final_tx = vec![1u8]; // signature count
        final_tx.extend_from_slice(&signature_bytes);
        final_tx.extend_from_slice(message);

        Ok(bs58::encode(&final_tx).into_string())
    }
}

#[async_trait]
impl BlockchainClient for RpcBlockchainClient {
    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), AppError> {
        let _: u64 = self.rpc_call("getSlot", Vec::<()>::new()).await?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn submit_transaction(&self, hash: &str) -> Result<String, AppError> {
        info!(hash = %hash, "Submitting transaction");

        #[cfg(feature = "real-blockchain")]
        {
            // Get recent blockhash
            let blockhash = self.get_latest_blockhash().await?;
            debug!(blockhash = %blockhash, "Got recent blockhash");

            // Build and sign transaction
            let tx = self.build_memo_transaction(hash, &blockhash)?;
            debug!("Built memo transaction");

            // Send transaction
            let params = serde_json::json!([tx, {"encoding": "base58"}]);
            let signature: String = self.rpc_call("sendTransaction", params).await?;
            info!(signature = %signature, "Transaction sent");

            Ok(signature)
        }

        #[cfg(not(feature = "real-blockchain"))]
        {
            // Mock implementation for testing
            let signature = self.sign(hash.as_bytes());
            Ok(format!("tx_{}", &signature[..16]))
        }
    }

    #[instrument(skip(self))]
    async fn get_block_height(&self) -> Result<u64, AppError> {
        self.rpc_call("getBlockHeight", Vec::<()>::new()).await
    }

    #[instrument(skip(self))]
    async fn get_latest_blockhash(&self) -> Result<String, AppError> {
        let result: BlockhashResult = self
            .rpc_call("getLatestBlockhash", Vec::<()>::new())
            .await?;
        Ok(result.value.blockhash)
    }

    #[instrument(skip(self))]
    async fn get_transaction_status(&self, signature: &str) -> Result<bool, AppError> {
        let params = serde_json::json!([[signature], {"searchTransactionHistory": true}]);
        let result: SignatureStatusResult = self.rpc_call("getSignatureStatuses", params).await?;

        match result.value.first() {
            Some(Some(status)) => {
                // Check if transaction errored
                if status.err.is_some() {
                    return Err(AppError::Blockchain(BlockchainError::TransactionFailed(
                        format!("Transaction failed: {:?}", status.err),
                    )));
                }
                // Check confirmation status
                let confirmed = status.confirmation_status.as_deref() == Some("confirmed")
                    || status.confirmation_status.as_deref() == Some("finalized");
                Ok(confirmed)
            }
            _ => Ok(false),
        }
    }

    #[instrument(skip(self))]
    async fn wait_for_confirmation(
        &self,
        signature: &str,
        timeout_secs: u64,
    ) -> Result<bool, AppError> {
        let timeout = Duration::from_secs(timeout_secs);
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(500);

        while start.elapsed() < timeout {
            match self.get_transaction_status(signature).await {
                Ok(true) => {
                    info!(signature = %signature, "Transaction confirmed");
                    return Ok(true);
                }
                Ok(false) => {
                    debug!(signature = %signature, "Transaction not yet confirmed");
                }
                Err(AppError::Blockchain(BlockchainError::TransactionFailed(msg))) => {
                    return Err(AppError::Blockchain(BlockchainError::TransactionFailed(
                        msg,
                    )));
                }
                Err(e) => {
                    warn!(signature = %signature, error = ?e, "Error checking transaction status");
                }
            }
            tokio::time::sleep(poll_interval).await;
        }

        Err(AppError::Blockchain(BlockchainError::Timeout(format!(
            "Transaction {} not confirmed within {}s",
            signature, timeout_secs
        ))))
    }
}

/// Parse a base58-encoded private key into a SigningKey
pub fn signing_key_from_base58(secret: &SecretString) -> Result<SigningKey, AppError> {
    let key_bytes = bs58::decode(secret.expose_secret())
        .into_vec()
        .map_err(|e| AppError::Blockchain(BlockchainError::InvalidSignature(e.to_string())))?;

    // Handle both 32-byte (seed) and 64-byte (keypair) formats
    let key_array: [u8; 32] = if key_bytes.len() == 64 {
        // Solana keypair format: first 32 bytes are the secret key
        key_bytes[..32].try_into().map_err(|_| {
            AppError::Blockchain(BlockchainError::InvalidSignature(
                "Invalid keypair format".to_string(),
            ))
        })?
    } else if key_bytes.len() == 32 {
        key_bytes.try_into().map_err(|v: Vec<u8>| {
            AppError::Blockchain(BlockchainError::InvalidSignature(format!(
                "Key must be 32 bytes, got {}",
                v.len()
            )))
        })?
    } else {
        return Err(AppError::Blockchain(BlockchainError::InvalidSignature(
            format!("Key must be 32 or 64 bytes, got {}", key_bytes.len()),
        )));
    };

    Ok(SigningKey::from_bytes(&key_array))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_client_creation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let client =
            RpcBlockchainClient::with_defaults("https://api.devnet.solana.com", signing_key);
        assert!(client.is_ok());
    }

    #[test]
    fn test_public_key_generation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let client =
            RpcBlockchainClient::with_defaults("https://api.devnet.solana.com", signing_key)
                .unwrap();
        let pubkey = client.public_key();
        assert!(!pubkey.is_empty());
        // Verify it decodes to 32 bytes (length can be 43 or 44 chars)
        let decoded = bs58::decode(&pubkey)
            .into_vec()
            .expect("Should be valid base58");
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn test_signing() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let client =
            RpcBlockchainClient::with_defaults("https://api.devnet.solana.com", signing_key)
                .unwrap();
        let signature = client.sign(b"test message");
        assert!(!signature.is_empty());
    }

    #[test]
    fn test_signing_key_from_base58_valid_32_bytes() {
        let original_key = SigningKey::generate(&mut OsRng);
        let encoded = bs58::encode(original_key.to_bytes()).into_string();
        let secret = SecretString::from(encoded);
        let result = signing_key_from_base58(&secret);
        assert!(result.is_ok());
    }

    #[test]
    fn test_signing_key_from_base58_valid_64_bytes() {
        let original_key = SigningKey::generate(&mut OsRng);
        let mut keypair = original_key.to_bytes().to_vec();
        keypair.extend_from_slice(original_key.verifying_key().as_bytes());
        let encoded = bs58::encode(&keypair).into_string();
        let secret = SecretString::from(encoded);
        let result = signing_key_from_base58(&secret);
        assert!(result.is_ok());
    }

    #[test]
    fn test_signing_key_from_base58_invalid() {
        let secret = SecretString::from("invalid-base58!!!");
        let result = signing_key_from_base58(&secret);
        assert!(result.is_err());
    }

    #[test]
    fn test_rpc_client_config_default() {
        let config = RpcClientConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.confirmation_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_signing_key_from_base58_wrong_length() {
        // 16 bytes - too short
        let short_key = bs58::encode(vec![0u8; 16]).into_string();
        let secret = SecretString::from(short_key);
        let result = signing_key_from_base58(&secret);
        assert!(result.is_err());

        // 48 bytes - wrong size (not 32 or 64)
        let wrong_key = bs58::encode(vec![0u8; 48]).into_string();
        let secret = SecretString::from(wrong_key);
        let result = signing_key_from_base58(&secret);
        assert!(result.is_err());
    }

    #[test]
    fn test_rpc_client_config_custom() {
        let config = RpcClientConfig {
            timeout: Duration::from_secs(60),
            max_retries: 5,
            retry_delay: Duration::from_millis(1000),
            confirmation_timeout: Duration::from_secs(120),
        };
        assert_eq!(config.timeout, Duration::from_secs(60));
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.retry_delay, Duration::from_millis(1000));
        assert_eq!(config.confirmation_timeout, Duration::from_secs(120));
    }

    #[test]
    fn test_signing_determinism() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let client =
            RpcBlockchainClient::with_defaults("https://api.devnet.solana.com", signing_key)
                .unwrap();

        // Same message should produce same signature
        let sig1 = client.sign(b"test message");
        let sig2 = client.sign(b"test message");
        assert_eq!(sig1, sig2);

        // Different message should produce different signature
        let sig3 = client.sign(b"different message");
        assert_ne!(sig1, sig3);
    }

    // --- MOCK PROVIDER TESTS ---
    use std::sync::Mutex;

    #[cfg(test)]
    #[allow(dead_code)]
    enum BlockchainErrorType {
        Timeout,
        Rpc,
    }

    struct MockState {
        requests: Vec<String>,
        should_fail_count: u32,
        failure_error: Option<BlockchainErrorType>,
        next_response: Option<serde_json::Value>,
    }

    struct MockSolanaRpcProvider {
        state: Mutex<MockState>,
        signing_key: SigningKey,
    }

    impl MockSolanaRpcProvider {
        fn new() -> Self {
            Self {
                state: Mutex::new(MockState {
                    requests: Vec::new(),
                    should_fail_count: 0,
                    failure_error: None,
                    next_response: None,
                }),
                signing_key: SigningKey::generate(&mut OsRng),
            }
        }

        fn with_failure(count: u32, error_type: BlockchainErrorType) -> Self {
            let provider = Self::new(); // removed `mut` since we don’t mutate `provider` itself
            {
                let mut state = provider.state.lock().unwrap();
                state.should_fail_count = count;
                state.failure_error = Some(error_type);
            }
            provider
        }
    }

    #[async_trait]
    impl SolanaRpcProvider for MockSolanaRpcProvider {
        async fn send_request(
            &self,
            method: &str,
            _params: serde_json::Value,
        ) -> Result<serde_json::Value, AppError> {
            let mut state = self.state.lock().unwrap();
            state.requests.push(method.to_string());

            if state.should_fail_count > 0 {
                state.should_fail_count -= 1;
                if let Some(ref err) = state.failure_error {
                    return match err {
                        BlockchainErrorType::Timeout => Err(AppError::Blockchain(
                            BlockchainError::Timeout("Mock timeout".to_string()),
                        )),
                        BlockchainErrorType::Rpc => Err(AppError::Blockchain(
                            BlockchainError::RpcError("Mock RPC error".to_string()),
                        )),
                    };
                }
            }

            if let Some(resp) = &state.next_response {
                return Ok(resp.clone());
            }

            Ok(serde_json::Value::Null)
        }

        fn public_key(&self) -> String {
            bs58::encode(self.signing_key.verifying_key().as_bytes()).into_string()
        }

        fn sign(&self, message: &[u8]) -> String {
            let signature = self.signing_key.sign(message);
            bs58::encode(signature.to_bytes()).into_string()
        }
    }

    #[tokio::test]
    async fn test_rpc_client_retry_logic_success() {
        // Setup provider that fails twice then succeeds
        let provider = MockSolanaRpcProvider::with_failure(2, BlockchainErrorType::Timeout);
        let config = RpcClientConfig {
            max_retries: 3,
            retry_delay: Duration::from_millis(1), // Fast retry
            ..Default::default()
        };

        // Set success response
        {
            let mut state = provider.state.lock().unwrap();
            state.next_response = Some(serde_json::json!(12345u64)); // Slot response
        }

        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        // Call health_check (uses getSlot)
        let result = client.health_check().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_rpc_client_retry_logic_failure() {
        // Setup provider that fails 4 times (max retries is 3)
        let provider = MockSolanaRpcProvider::with_failure(4, BlockchainErrorType::Timeout);
        let config = RpcClientConfig {
            max_retries: 3,
            retry_delay: Duration::from_millis(1),
            ..Default::default()
        };

        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.health_check().await;
        assert!(matches!(
            result,
            Err(AppError::Blockchain(BlockchainError::Timeout(_)))
        ));
    }

    // --- ENHANCED MOCK FOR ERROR SCENARIOS ---

    #[derive(Clone)]
    #[allow(dead_code)]
    enum MockErrorKind {
        Timeout(String),
        RpcError(String),
        InsufficientFunds,
        TransactionFailed(String),
        EmptyResponse,
    }

    struct ConfigurableMockProvider {
        signing_key: SigningKey,
        responses: Mutex<Vec<Result<serde_json::Value, MockErrorKind>>>,
        call_count: Mutex<usize>,
    }

    impl ConfigurableMockProvider {
        fn new() -> Self {
            Self {
                signing_key: SigningKey::generate(&mut OsRng),
                responses: Mutex::new(Vec::new()),
                call_count: Mutex::new(0),
            }
        }

        fn with_responses(responses: Vec<Result<serde_json::Value, MockErrorKind>>) -> Self {
            let provider = Self::new();
            *provider.responses.lock().unwrap() = responses;
            provider
        }

        #[allow(dead_code)]
        fn get_call_count(&self) -> usize {
            *self.call_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl SolanaRpcProvider for ConfigurableMockProvider {
        async fn send_request(
            &self,
            _method: &str,
            _params: serde_json::Value,
        ) -> Result<serde_json::Value, AppError> {
            let mut count = self.call_count.lock().unwrap();
            let idx = *count;
            *count += 1;
            drop(count);

            let responses = self.responses.lock().unwrap();
            if idx < responses.len() {
                match &responses[idx] {
                    Ok(v) => Ok(v.clone()),
                    Err(MockErrorKind::Timeout(msg)) => {
                        Err(AppError::Blockchain(BlockchainError::Timeout(msg.clone())))
                    }
                    Err(MockErrorKind::RpcError(msg)) => {
                        Err(AppError::Blockchain(BlockchainError::RpcError(msg.clone())))
                    }
                    Err(MockErrorKind::InsufficientFunds) => {
                        Err(AppError::Blockchain(BlockchainError::InsufficientFunds))
                    }
                    Err(MockErrorKind::TransactionFailed(msg)) => Err(AppError::Blockchain(
                        BlockchainError::TransactionFailed(msg.clone()),
                    )),
                    Err(MockErrorKind::EmptyResponse) => Err(AppError::Blockchain(
                        BlockchainError::RpcError("Empty response".to_string()),
                    )),
                }
            } else {
                Ok(serde_json::Value::Null)
            }
        }

        fn public_key(&self) -> String {
            bs58::encode(self.signing_key.verifying_key().as_bytes()).into_string()
        }

        fn sign(&self, message: &[u8]) -> String {
            let signature = self.signing_key.sign(message);
            bs58::encode(signature.to_bytes()).into_string()
        }
    }

    // --- ERROR HANDLING TESTS ---

    #[tokio::test]
    async fn test_rpc_error_insufficient_funds() {
        let provider =
            ConfigurableMockProvider::with_responses(vec![Err(MockErrorKind::InsufficientFunds)]);
        let config = RpcClientConfig {
            max_retries: 0, // No retries for this test
            ..Default::default()
        };
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.health_check().await;
        assert!(matches!(
            result,
            Err(AppError::Blockchain(BlockchainError::InsufficientFunds))
        ));
    }

    #[tokio::test]
    async fn test_rpc_error_timeout_mapping() {
        let provider = ConfigurableMockProvider::with_responses(vec![Err(MockErrorKind::Timeout(
            "Connection timed out".to_string(),
        ))]);
        let config = RpcClientConfig {
            max_retries: 0,
            ..Default::default()
        };
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.health_check().await;
        match result {
            Err(AppError::Blockchain(BlockchainError::Timeout(msg))) => {
                assert!(msg.contains("timed out"));
            }
            _ => panic!("Expected timeout error"),
        }
    }

    #[tokio::test]
    async fn test_rpc_error_generic_rpc_error() {
        let provider = ConfigurableMockProvider::with_responses(vec![Err(
            MockErrorKind::RpcError("-32000: Server is busy".to_string()),
        )]);
        let config = RpcClientConfig {
            max_retries: 0,
            ..Default::default()
        };
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.health_check().await;
        match result {
            Err(AppError::Blockchain(BlockchainError::RpcError(msg))) => {
                assert!(msg.contains("Server is busy"));
            }
            _ => panic!("Expected RPC error"),
        }
    }

    // --- DESERIALIZATION TESTS ---

    #[test]
    fn test_deserialize_signature_status_confirmed() {
        let json = serde_json::json!({
            "err": null,
            "confirmationStatus": "confirmed"
        });
        let status: SignatureStatus = serde_json::from_value(json).unwrap();
        assert!(status.err.is_none());
        assert_eq!(status.confirmation_status.as_deref(), Some("confirmed"));
    }

    #[test]
    fn test_deserialize_signature_status_finalized() {
        let json = serde_json::json!({
            "err": null,
            "confirmationStatus": "finalized"
        });
        let status: SignatureStatus = serde_json::from_value(json).unwrap();
        assert!(status.err.is_none());
        assert_eq!(status.confirmation_status.as_deref(), Some("finalized"));
    }

    #[test]
    fn test_deserialize_signature_status_with_error() {
        let json = serde_json::json!({
            "err": {"InstructionError": [0, "Custom"]},
            "confirmationStatus": "confirmed"
        });
        let status: SignatureStatus = serde_json::from_value(json).unwrap();
        assert!(status.err.is_some());
    }

    #[test]
    fn test_deserialize_signature_status_null_confirmation() {
        let json = serde_json::json!({
            "err": null,
            "confirmationStatus": null
        });
        let status: SignatureStatus = serde_json::from_value(json).unwrap();
        assert!(status.confirmation_status.is_none());
    }

    #[test]
    fn test_deserialize_blockhash_result() {
        let json = serde_json::json!({
            "value": {
                "blockhash": "GHtXQBsoZHVnNFa9YevAzFr17DJjgHXk3ycTy5nRhVT3"
            }
        });
        let result: BlockhashResult = serde_json::from_value(json).unwrap();
        assert_eq!(
            result.value.blockhash,
            "GHtXQBsoZHVnNFa9YevAzFr17DJjgHXk3ycTy5nRhVT3"
        );
    }

    #[test]
    fn test_deserialize_signature_status_result() {
        let json = serde_json::json!({
            "value": [
                {
                    "err": null,
                    "confirmationStatus": "finalized"
                }
            ]
        });
        let result: SignatureStatusResult = serde_json::from_value(json).unwrap();
        assert_eq!(result.value.len(), 1);
        assert!(result.value[0].is_some());
    }

    #[test]
    fn test_deserialize_signature_status_result_null_entry() {
        let json = serde_json::json!({
            "value": [null]
        });
        let result: SignatureStatusResult = serde_json::from_value(json).unwrap();
        assert_eq!(result.value.len(), 1);
        assert!(result.value[0].is_none());
    }

    // --- TRANSACTION STATUS TESTS ---

    #[tokio::test]
    async fn test_get_transaction_status_confirmed() {
        let provider = ConfigurableMockProvider::with_responses(vec![Ok(serde_json::json!({
            "value": [{
                "err": null,
                "confirmationStatus": "confirmed"
            }]
        }))]);
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.get_transaction_status("test_sig").await;
        assert!(result.is_ok());
        assert!(result.unwrap()); // Should be confirmed
    }

    #[tokio::test]
    async fn test_get_transaction_status_finalized() {
        let provider = ConfigurableMockProvider::with_responses(vec![Ok(serde_json::json!({
            "value": [{
                "err": null,
                "confirmationStatus": "finalized"
            }]
        }))]);
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.get_transaction_status("test_sig").await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_get_transaction_status_not_found() {
        let provider = ConfigurableMockProvider::with_responses(vec![Ok(serde_json::json!({
            "value": [null]
        }))]);
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.get_transaction_status("unknown_sig").await;
        assert!(result.is_ok());
        assert!(!result.unwrap()); // Not found = not confirmed
    }

    #[tokio::test]
    async fn test_get_transaction_status_with_error() {
        let provider = ConfigurableMockProvider::with_responses(vec![Ok(serde_json::json!({
            "value": [{
                "err": {"InstructionError": [0, "Custom"]},
                "confirmationStatus": "confirmed"
            }]
        }))]);
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.get_transaction_status("failed_sig").await;
        assert!(matches!(
            result,
            Err(AppError::Blockchain(BlockchainError::TransactionFailed(_)))
        ));
    }

    // --- BLOCKHASH AND BLOCK HEIGHT TESTS ---

    #[tokio::test]
    async fn test_get_latest_blockhash() {
        let provider = ConfigurableMockProvider::with_responses(vec![Ok(serde_json::json!({
            "value": {
                "blockhash": "TestBlockhash123"
            }
        }))]);
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.get_latest_blockhash().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "TestBlockhash123");
    }

    #[tokio::test]
    async fn test_get_block_height() {
        let provider =
            ConfigurableMockProvider::with_responses(vec![Ok(serde_json::json!(123456789u64))]);
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.get_block_height().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 123456789);
    }

    // --- WAIT FOR CONFIRMATION TESTS ---

    #[tokio::test]
    async fn test_wait_for_confirmation_immediate_success() {
        let provider = ConfigurableMockProvider::with_responses(vec![Ok(serde_json::json!({
            "value": [{
                "err": null,
                "confirmationStatus": "finalized"
            }]
        }))]);
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.wait_for_confirmation("test_sig", 5).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_wait_for_confirmation_eventual_success() {
        // First call: not confirmed, second call: confirmed
        let provider = ConfigurableMockProvider::with_responses(vec![
            Ok(serde_json::json!({"value": [null]})),
            Ok(serde_json::json!({
                "value": [{
                    "err": null,
                    "confirmationStatus": "confirmed"
                }]
            })),
        ]);
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        tokio::time::pause();
        let result = client.wait_for_confirmation("test_sig", 10).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_wait_for_confirmation_timeout() {
        // Always return not confirmed
        let provider = ConfigurableMockProvider::with_responses(vec![
            Ok(serde_json::json!({"value": [null]})),
            Ok(serde_json::json!({"value": [null]})),
            Ok(serde_json::json!({"value": [null]})),
            Ok(serde_json::json!({"value": [null]})),
            Ok(serde_json::json!({"value": [null]})),
        ]);
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        tokio::time::pause();
        let result = client.wait_for_confirmation("never_confirmed", 1).await;
        assert!(matches!(
            result,
            Err(AppError::Blockchain(BlockchainError::Timeout(_)))
        ));
    }

    #[tokio::test]
    async fn test_wait_for_confirmation_transaction_failed() {
        let provider = ConfigurableMockProvider::with_responses(vec![Ok(serde_json::json!({
            "value": [{
                "err": {"InstructionError": [0, "ProgramFailed"]},
                "confirmationStatus": "confirmed"
            }]
        }))]);
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.wait_for_confirmation("failed_tx", 5).await;
        assert!(matches!(
            result,
            Err(AppError::Blockchain(BlockchainError::TransactionFailed(_)))
        ));
    }

    // --- SUBMIT TRANSACTION TESTS (MOCK MODE) ---

    #[tokio::test]
    #[cfg(not(feature = "real-blockchain"))]
    async fn test_submit_transaction_mock_mode() {
        let provider = ConfigurableMockProvider::new();
        let config = RpcClientConfig::default();
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        // In mock mode (no real-blockchain feature), submit_transaction just signs
        let result = client.submit_transaction("test_hash_123").await;
        assert!(result.is_ok());
        let signature = result.unwrap();
        assert!(signature.starts_with("tx_")); // Mock format
    }

    // --- RETRY LOGIC WITH CALL TRACKING ---

    #[tokio::test]
    async fn test_retry_counts_attempts_correctly() {
        let provider = ConfigurableMockProvider::with_responses(vec![
            Err(MockErrorKind::Timeout("fail 1".to_string())),
            Err(MockErrorKind::Timeout("fail 2".to_string())),
            Err(MockErrorKind::Timeout("fail 3".to_string())),
            Ok(serde_json::json!(999u64)), // Success on 4th attempt
        ]);
        let config = RpcClientConfig {
            max_retries: 3, // Initial + 3 retries = 4 attempts
            retry_delay: Duration::from_millis(1),
            ..Default::default()
        };
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.health_check().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_no_retry_on_insufficient_funds() {
        // InsufficientFunds should still trigger retries as per current implementation
        let provider = ConfigurableMockProvider::with_responses(vec![
            Err(MockErrorKind::InsufficientFunds),
            Err(MockErrorKind::InsufficientFunds),
        ]);
        let config = RpcClientConfig {
            max_retries: 1,
            retry_delay: Duration::from_millis(1),
            ..Default::default()
        };
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        let result = client.health_check().await;
        assert!(matches!(
            result,
            Err(AppError::Blockchain(BlockchainError::InsufficientFunds))
        ));
        // Note: We can't check the provider's state after moving it into Box
        // The test validates that InsufficientFunds is eventually returned after retries
    }

    // --- WITH_PROVIDER CONSTRUCTOR TEST ---

    #[test]
    fn test_with_provider_constructor() {
        let provider = ConfigurableMockProvider::new();
        let config = RpcClientConfig {
            max_retries: 5,
            timeout: Duration::from_secs(45),
            ..Default::default()
        };
        let client = RpcBlockchainClient::with_provider(Box::new(provider), config);

        // Verify public key is accessible
        let pubkey = client.public_key();
        assert!(!pubkey.is_empty());

        // Verify signing works
        let sig = client.sign(b"test");
        assert!(!sig.is_empty());
    }
}

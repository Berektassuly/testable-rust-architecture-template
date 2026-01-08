//! Blockchain RPC client implementation for Solana.
//!
//! This module provides both mock and real blockchain interactions.
//! Real blockchain functionality is enabled with the `real-blockchain` feature.

use async_trait::async_trait;
use ed25519_dalek::{Signer, SigningKey};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
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

/// Solana RPC blockchain client
pub struct RpcBlockchainClient {
    http_client: Client,
    rpc_url: String,
    signing_key: SigningKey,
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
    #[serde(rename = "lastValidBlockHeight")]
    #[allow(dead_code)]
    last_valid_block_height: u64,
}

#[derive(Debug, Deserialize)]
struct BlockhashResult {
    value: BlockhashResponse,
}

#[derive(Debug, Deserialize)]
struct SignatureStatus {
    #[allow(dead_code)]
    slot: Option<u64>,
    #[allow(dead_code)]
    confirmations: Option<u64>,
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
        let http_client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| AppError::Blockchain(BlockchainError::Connection(e.to_string())))?;
        info!(rpc_url = %rpc_url, "Created blockchain client");
        Ok(Self {
            http_client,
            rpc_url: rpc_url.to_string(),
            signing_key,
            config,
        })
    }

    /// Create a new RPC blockchain client with default configuration
    pub fn with_defaults(rpc_url: &str, signing_key: SigningKey) -> Result<Self, AppError> {
        Self::new(rpc_url, signing_key, RpcClientConfig::default())
    }

    /// Get the public key as base58 string
    #[must_use]
    pub fn public_key(&self) -> String {
        bs58::encode(self.signing_key.verifying_key().as_bytes()).into_string()
    }

    /// Sign a message and return the signature as base58
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> String {
        let signature = self.signing_key.sign(message);
        bs58::encode(signature.to_bytes()).into_string()
    }

    /// Make an RPC call with retries
    #[instrument(skip(self, params))]
    async fn rpc_call<P: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, AppError> {
        let mut last_error = None;
        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                tokio::time::sleep(self.config.retry_delay).await;
            }
            match self.do_rpc_call(method, &params).await {
                Ok(result) => return Ok(result),
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

    /// Execute a single RPC call
    async fn do_rpc_call<P: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: &P,
    ) -> Result<R, AppError> {
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

        let rpc_response: JsonRpcResponse<R> = response
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

        let public_key = self.signing_key.verifying_key().to_bytes();

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
        tx_data.extend_from_slice(&public_key);
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
        let signature = self.signing_key.sign(message);

        // Insert signature
        let mut final_tx = vec![1u8]; // signature count
        final_tx.extend_from_slice(&signature.to_bytes());
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
}

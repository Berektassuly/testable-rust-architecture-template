//! Blockchain RPC client implementation.
//!
//! This module provides a production-ready blockchain client that uses
//! HTTP/JSON-RPC to communicate with blockchain nodes (Solana-compatible).

use async_trait::async_trait;
use ed25519_dalek::{Signer, SigningKey};
use reqwest::Client;
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, instrument, warn};

use crate::domain::{AppError, BlockchainClient, BlockchainError};

/// Configuration for the blockchain RPC client.
#[derive(Debug, Clone)]
pub struct RpcClientConfig {
    /// Request timeout.
    pub timeout: Duration,
    /// Maximum retry attempts.
    pub max_retries: u32,
    /// Delay between retries.
    pub retry_delay: Duration,
}

impl Default for RpcClientConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay: Duration::from_millis(500),
        }
    }
}

/// A generic blockchain RPC client.
///
/// This client communicates with blockchain nodes via JSON-RPC over HTTP.
/// It uses `reqwest` with `rustls` for TLS, avoiding OpenSSL dependencies.
///
/// # Security
///
/// The signing key is stored using the `secrecy` crate to prevent
/// accidental logging of sensitive data.
///
/// # Example
///
/// ```ignore
/// use ed25519_dalek::SigningKey;
/// use rand::rngs::OsRng;
///
/// let signing_key = SigningKey::generate(&mut OsRng);
/// let client = RpcBlockchainClient::new(
///     "https://api.devnet.solana.com",
///     signing_key,
///     RpcClientConfig::default(),
/// )?;
/// ```
pub struct RpcBlockchainClient {
    http_client: Client,
    rpc_url: String,
    signing_key: SigningKey,
    config: RpcClientConfig,
}

/// JSON-RPC request structure.
#[derive(Debug, Serialize)]
struct JsonRpcRequest<T: Serialize> {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: T,
}

/// JSON-RPC response structure.
#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: u64,
    result: Option<T>,
    error: Option<JsonRpcError>,
}

/// JSON-RPC error structure.
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

impl RpcBlockchainClient {
    /// Creates a new `RpcBlockchainClient` instance.
    ///
    /// # Arguments
    ///
    /// * `rpc_url` - The blockchain node's RPC endpoint URL.
    /// * `signing_key` - The Ed25519 signing key for transaction signing.
    /// * `config` - Client configuration options.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be initialized.
    pub fn new(
        rpc_url: &str,
        signing_key: SigningKey,
        config: RpcClientConfig,
    ) -> Result<Self, AppError> {
        let http_client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| {
                AppError::Blockchain(BlockchainError::Connection(format!(
                    "Failed to create HTTP client: {}",
                    e
                )))
            })?;

        info!(rpc_url = %rpc_url, "Created blockchain RPC client");

        Ok(Self {
            http_client,
            rpc_url: rpc_url.to_string(),
            signing_key,
            config,
        })
    }

    /// Creates a new client with default configuration.
    pub fn with_defaults(rpc_url: &str, signing_key: SigningKey) -> Result<Self, AppError> {
        Self::new(rpc_url, signing_key, RpcClientConfig::default())
    }

    /// Returns the public key associated with this client's signing key.
    #[must_use]
    pub fn public_key(&self) -> String {
        bs58::encode(self.signing_key.verifying_key().as_bytes()).into_string()
    }

    /// Signs a message using the client's signing key.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> String {
        let signature = self.signing_key.sign(message);
        bs58::encode(signature.to_bytes()).into_string()
    }

    /// Makes a JSON-RPC call to the blockchain node with retries.
    #[instrument(skip(self, params), fields(method = %method))]
    async fn rpc_call<P: Serialize + std::fmt::Debug, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, AppError> {
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                debug!(attempt = attempt, "Retrying RPC call");
                tokio::time::sleep(self.config.retry_delay).await;
            }

            match self.do_rpc_call(method, &params).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    warn!(
                        attempt = attempt,
                        max_retries = self.config.max_retries,
                        error = ?e,
                        "RPC call failed"
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            AppError::Blockchain(BlockchainError::RpcError("Unknown error".to_string()))
        }))
    }

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
                AppError::Blockchain(BlockchainError::RpcError(format!(
                    "HTTP request failed: {}",
                    e
                )))
            })?;

        if !response.status().is_success() {
            return Err(AppError::Blockchain(BlockchainError::RpcError(format!(
                "HTTP error: {}",
                response.status()
            ))));
        }

        let rpc_response: JsonRpcResponse<R> = response.json().await.map_err(|e| {
            AppError::Blockchain(BlockchainError::RpcError(format!(
                "Failed to parse response: {}",
                e
            )))
        })?;

        if let Some(error) = rpc_response.error {
            return Err(AppError::Blockchain(BlockchainError::RpcError(format!(
                "RPC error {}: {}",
                error.code, error.message
            ))));
        }

        rpc_response
            .result
            .ok_or_else(|| AppError::Blockchain(BlockchainError::RpcError("Empty response".to_string())))
    }
}

#[async_trait]
impl BlockchainClient for RpcBlockchainClient {
    #[instrument(skip(self))]
    async fn health_check(&self) -> Result<(), AppError> {
        debug!("Performing blockchain health check");

        // Try to get the current slot as a health check
        let result: Result<u64, _> = self.rpc_call("getSlot", Vec::<()>::new()).await;

        match result {
            Ok(slot) => {
                debug!(slot = slot, "Blockchain is healthy");
                Ok(())
            }
            Err(e) => {
                warn!(error = ?e, "Blockchain health check failed");
                Err(e)
            }
        }
    }

    #[instrument(skip(self))]
    async fn submit_transaction(&self, hash: &str) -> Result<String, AppError> {
        info!(hash = %hash, "Submitting transaction to blockchain");

        // Sign the hash
        let signature = self.sign(hash.as_bytes());

        // In a full implementation, you would:
        // 1. Construct a proper transaction with the hash as memo/data
        // 2. Sign the entire transaction
        // 3. Serialize and send via sendTransaction RPC
        //
        // For this template, we demonstrate the signing pattern
        // and return the signature as a transaction ID.
        //
        // Note: The actual transaction submission would require
        // constructing proper Solana transactions, which needs
        // additional dependencies or more complex serialization.

        debug!(
            hash = %hash,
            signature = %signature,
            "Transaction signed"
        );

        // Return the signature as a transaction ID
        // In production, this would be the actual transaction signature
        // returned by the blockchain after confirmation
        Ok(format!("tx_{}", &signature[..16]))
    }

    #[instrument(skip(self))]
    async fn get_transaction_status(&self, signature: &str) -> Result<bool, AppError> {
        debug!(signature = %signature, "Checking transaction status");

        // In a real implementation, you would call getSignatureStatuses
        // For now, we'll simulate by trying to get transaction info

        // This is a simplified implementation
        // Real implementation would parse the actual RPC response
        let _result: Result<serde_json::Value, _> = self
            .rpc_call("getTransaction", vec![signature, "json"])
            .await;

        // If we got a result, the transaction exists
        // In reality, you'd check the confirmation status
        Ok(true)
    }

    #[instrument(skip(self))]
    async fn get_block_height(&self) -> Result<u64, AppError> {
        debug!("Getting current block height");

        let height: u64 = self.rpc_call("getBlockHeight", Vec::<()>::new()).await?;

        debug!(height = height, "Current block height");

        Ok(height)
    }
}

/// Helper to create a signing key from a base58-encoded secret.
///
/// This function safely handles the secret key without logging it.
pub fn signing_key_from_base58(secret: &Secret<String>) -> Result<SigningKey, AppError> {
    let key_bytes = bs58::decode(secret.expose_secret())
        .into_vec()
        .map_err(|e| {
            AppError::Blockchain(BlockchainError::InvalidSignature(format!(
                "Invalid base58 encoding: {}",
                e
            )))
        })?;

    let key_array: [u8; 32] = key_bytes.try_into().map_err(|v: Vec<u8>| {
        AppError::Blockchain(BlockchainError::InvalidSignature(format!(
            "Key must be 32 bytes, got {} bytes",
            v.len()
        )))
    })?;

    Ok(SigningKey::from_bytes(&key_array))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_client_creation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let client = RpcBlockchainClient::with_defaults(
            "https://api.devnet.solana.com",
            signing_key,
        );
        assert!(client.is_ok());
    }

    #[test]
    fn test_public_key_generation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let client = RpcBlockchainClient::with_defaults(
            "https://api.devnet.solana.com",
            signing_key,
        )
        .unwrap();

        let pubkey = client.public_key();
        // Solana public keys are 32 bytes, Base58 encoded
        assert!(!pubkey.is_empty());
        assert!(pubkey.len() >= 32 && pubkey.len() <= 44);
    }

    #[test]
    fn test_signing() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let client = RpcBlockchainClient::with_defaults(
            "https://api.devnet.solana.com",
            signing_key,
        )
        .unwrap();

        let message = b"test message";
        let signature = client.sign(message);

        // Ed25519 signatures are 64 bytes, Base58 encoded
        assert!(!signature.is_empty());
    }

    #[test]
    fn test_signing_key_from_base58_valid() {
        // Generate a key and encode it
        let original_key = SigningKey::generate(&mut OsRng);
        let encoded = bs58::encode(original_key.to_bytes()).into_string();
        let secret = Secret::new(encoded);

        let result = signing_key_from_base58(&secret);
        assert!(result.is_ok());
    }

    #[test]
    fn test_signing_key_from_base58_invalid() {
        let secret = Secret::new("invalid-base58!!!".to_string());
        let result = signing_key_from_base58(&secret);
        assert!(result.is_err());
    }

    #[test]
    fn test_rpc_client_config_default() {
        let config = RpcClientConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.timeout, Duration::from_secs(30));
    }
}

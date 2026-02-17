//! Transaction signer strategies: local key (dev/legacy) and KMS placeholder.
//!
//! Decouples signing from the RPC client so that raw private keys are not held
//! in the client and remote signers (HSM, AWS KMS, Vault) can be used.

use async_trait::async_trait;
use ed25519_dalek::{Signer, SigningKey};
use secrecy::{ExposeSecret, SecretString};
use tracing::info;

use crate::domain::{BlockchainError, TransactionSigner};

/// Parse base58-encoded private key into a SigningKey. Used only within local scope.
fn signing_key_from_secret(secret: &SecretString) -> Result<SigningKey, BlockchainError> {
    let key_bytes = bs58::decode(secret.expose_secret())
        .into_vec()
        .map_err(|e| BlockchainError::SubmissionFailed(e.to_string()))?;

    let key_array: [u8; 32] = if key_bytes.len() == 64 {
        key_bytes[..32]
            .try_into()
            .map_err(|_| BlockchainError::SubmissionFailed("Invalid keypair format".to_string()))?
    } else if key_bytes.len() == 32 {
        key_bytes.try_into().map_err(|v: Vec<u8>| {
            BlockchainError::SubmissionFailed(format!("Key must be 32 bytes, got {}", v.len()))
        })?
    } else {
        return Err(BlockchainError::SubmissionFailed(format!(
            "Key must be 32 or 64 bytes, got {}",
            key_bytes.len()
        )));
    };

    Ok(SigningKey::from_bytes(&key_array))
}

/// Local signer (dev/legacy): holds secret in memory, parses only when signing.
/// Raw secret is exposed only in the scope of `sign_message`.
pub struct LocalSigner {
    secret: SecretString,
    public_key_base58: String,
}

impl LocalSigner {
    /// Build a local signer from a Base58-encoded secret (32-byte seed or 64-byte keypair).
    pub fn new(secret: SecretString) -> Result<Self, BlockchainError> {
        let signing_key = signing_key_from_secret(&secret)?;
        let public_key_base58 = bs58::encode(signing_key.verifying_key().as_bytes()).into_string();
        Ok(Self {
            secret,
            public_key_base58,
        })
    }
}

#[async_trait]
impl TransactionSigner for LocalSigner {
    async fn sign_message(&self, message: &[u8]) -> Result<String, BlockchainError> {
        let signing_key = signing_key_from_secret(&self.secret)?;
        let signature = signing_key.sign(message);
        Ok(bs58::encode(signature.to_bytes()).into_string())
    }

    fn public_key(&self) -> String {
        self.public_key_base58.clone()
    }
}

/// AWS KMS signer (production target). Placeholder: no AWS SDK yet.
/// Establishes the architectural boundary; real KMS calls to be added later.
pub struct AwsKmsSigner {
    pub key_id: String,
    #[allow(clippy::type_complexity)]
    pub client: (),
}

impl AwsKmsSigner {
    /// Create an KMS signer for the given key ID. No AWS client yet; mock only.
    #[must_use]
    pub fn new(key_id: String) -> Self {
        Self { key_id, client: () }
    }
}

#[async_trait]
impl TransactionSigner for AwsKmsSigner {
    async fn sign_message(&self, message: &[u8]) -> Result<String, BlockchainError> {
        info!(
            key_id = %self.key_id,
            message_len = message.len(),
            "Mocking KMS call for key"
        );
        let _ = message;
        // Dummy base58 signature for architectural boundary without SDK dependency.
        Ok(bs58::encode(&[0u8; 64]).into_string())
    }

    fn public_key(&self) -> String {
        // Placeholder until KMS returns public key; same dummy as mock signature payload.
        bs58::encode(&[0u8; 32]).into_string()
    }
}

//! Transaction signer strategies: local key (dev/legacy) and AWS KMS (production).
//!
//! Decouples signing from the RPC client so that raw private keys are not held
//! in the client and remote signers (HSM, AWS KMS, Vault) can be used.

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_kms::primitives::Blob;
use aws_sdk_kms::types::{MessageType, SigningAlgorithmSpec};
use ed25519_dalek::{Signer, SigningKey};
use secrecy::{ExposeSecret, SecretString};
use tracing::{debug, info};

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

// ---------------------------------------------------------------------------
// AWS KMS Signer — production remote signing via Ed25519
// ---------------------------------------------------------------------------

/// Ed25519 SPKI DER header (RFC 8410).
///
/// ```text
/// SEQUENCE (2 elem, 0x30 0x2a)
///   SEQUENCE (1 elem, 0x30 0x05)
///     OID 1.3.101.112 (Ed25519, 0x06 0x03 0x2b 0x65 0x70)
///   BIT STRING (33 bytes, 0x03 0x21 0x00)
///     <32 bytes of raw public key>
/// ```
///
/// Total SPKI blob length = 12 (header) + 32 (key) = 44 bytes.
const ED25519_SPKI_HEADER: [u8; 12] = [
    0x30, 0x2a, // SEQUENCE, 42 bytes
    0x30, 0x05, // SEQUENCE, 5 bytes
    0x06, 0x03, 0x2b, 0x65, 0x70, // OID 1.3.101.112 (Ed25519)
    0x03, 0x21, 0x00, // BIT STRING, 33 bytes, 0 unused bits
];

/// AWS KMS signer (production). Performs remote Ed25519 signing.
///
/// The raw 32-byte public key is fetched once during construction via
/// `GetPublicKey` and cached as a Base58 string (Solana address).
pub struct AwsKmsSigner {
    client: aws_sdk_kms::Client,
    key_id: String,
    pubkey_base58: String,
}

impl AwsKmsSigner {
    /// Create a KMS signer for the given key ID.
    ///
    /// Loads AWS configuration from the environment (env vars, instance
    /// metadata, ECS task role, etc.), calls `GetPublicKey` to fetch and
    /// cache the Ed25519 public key, and validates the SPKI DER header.
    pub async fn new(key_id: String) -> Result<Self, BlockchainError> {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let client = aws_sdk_kms::Client::new(&config);

        info!(key_id = %key_id, "Initializing AWS KMS signer");

        // -- Fetch the public key from KMS --------------------------------
        let response = client
            .get_public_key()
            .key_id(&key_id)
            .send()
            .await
            .map_err(|e| {
                BlockchainError::SubmissionFailed(format!("KMS GetPublicKey failed: {e}"))
            })?;

        let spki_blob = response
            .public_key
            .ok_or_else(|| {
                BlockchainError::SubmissionFailed("KMS returned no public key blob".to_string())
            })?
            .into_inner();

        // -- Extract raw 32-byte Ed25519 key from the DER-encoded SPKI ----
        let raw_key = extract_ed25519_pubkey(&spki_blob)?;

        let pubkey_base58 = bs58::encode(raw_key).into_string();
        info!(public_key = %pubkey_base58, "KMS signer initialized");

        Ok(Self {
            client,
            key_id,
            pubkey_base58,
        })
    }
}

/// Extract the raw 32-byte Ed25519 public key from a DER-encoded SPKI blob.
///
/// Validates the ASN.1 header matches the expected Ed25519 OID.  Falls back
/// to taking the last 32 bytes when the blob has an unexpected length but
/// still contains enough data (logged at debug level for diagnostics).
fn extract_ed25519_pubkey(spki: &[u8]) -> Result<&[u8], BlockchainError> {
    if spki.len() == 44 {
        // Standard path: verify header then slice the trailing 32 bytes.
        if spki[..12] == ED25519_SPKI_HEADER {
            return Ok(&spki[12..]);
        }
        return Err(BlockchainError::SubmissionFailed(
            "SPKI header does not match Ed25519 OID (1.3.101.112)".to_string(),
        ));
    }

    // Non-standard path: some KMS responses may have padding / extra wrapping.
    if spki.len() >= 32 {
        debug!(
            blob_len = spki.len(),
            "SPKI blob is not 44 bytes; extracting last 32 bytes as raw key"
        );
        return Ok(&spki[spki.len() - 32..]);
    }

    Err(BlockchainError::SubmissionFailed(format!(
        "SPKI blob too short ({} bytes); expected ≥ 32",
        spki.len()
    )))
}

#[async_trait]
impl TransactionSigner for AwsKmsSigner {
    async fn sign_message(&self, message: &[u8]) -> Result<String, BlockchainError> {
        debug!(
            key_id = %self.key_id,
            message_len = message.len(),
            "Calling KMS Sign (Ed25519)"
        );

        let response = self
            .client
            .sign()
            .key_id(&self.key_id)
            .message(Blob::new(message))
            .message_type(MessageType::Raw)
            .signing_algorithm(SigningAlgorithmSpec::Ed25519)
            .send()
            .await
            .map_err(|e| BlockchainError::SubmissionFailed(format!("KMS Sign failed: {e}")))?;

        let signature_blob = response.signature.ok_or_else(|| {
            BlockchainError::SubmissionFailed("KMS returned no signature blob".to_string())
        })?;

        Ok(bs58::encode(signature_blob.into_inner()).into_string())
    }

    fn public_key(&self) -> String {
        self.pubkey_base58.clone()
    }
}

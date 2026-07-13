use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureEnvelope {
    pub algorithm: String,
    pub key_id: String,
    pub signature: Vec<u8>,
}

pub trait ManifestSigner: Send + Sync {
    fn algorithm(&self) -> &'static str;
    fn key_id(&self) -> &str;
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SigningError>;
    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), SigningError>;

    fn sign_envelope(&self, message: &[u8]) -> Result<SignatureEnvelope, SigningError> {
        Ok(SignatureEnvelope {
            algorithm: self.algorithm().to_owned(),
            key_id: self.key_id().to_owned(),
            signature: self.sign(message)?,
        })
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SigningError {
    #[error("manifest signing failed")]
    SignFailed,
    #[error("manifest signature verification failed")]
    VerificationFailed,
}

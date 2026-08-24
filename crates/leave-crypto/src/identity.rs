//! Long-term device identity and the key packages that invite it to a group.

use crate::error::{CryptoError, Result};
use crate::provider::LeaveProvider;
use openmls::prelude::{
    BasicCredential, Ciphersuite, CredentialWithKey, KeyPackage, KeyPackageBundle,
    MlsMessageBodyIn, MlsMessageIn, MlsMessageOut, ProtocolVersion,
    tls_codec::{Deserialize as TlsDeserialize, Serialize as TlsSerialize},
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_traits::OpenMlsProvider;

/// The ciphersuite every Leave workspace uses.
///
/// One mandatory ciphersuite keeps downgrade negotiation out of the protocol.
pub const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

/// One device's signing key, credential, and local key material.
///
/// A device identity never leaves the machine that generated it. Only the
/// public key package it publishes does.
pub struct DeviceIdentity {
    device_id: String,
    provider: LeaveProvider,
    signer: SignatureKeyPair,
    credential: CredentialWithKey,
}

impl DeviceIdentity {
    /// Generate a fresh signing key and credential for one device.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Identity`] when the provider cannot create or
    /// store the signature key pair.
    pub fn generate(device_id: &str) -> Result<Self> {
        let provider = LeaveProvider::default();
        let signer = SignatureKeyPair::new(CIPHERSUITE.signature_algorithm())
            .map_err(|error| CryptoError::Identity(error.to_string()))?;
        signer
            .store(provider.storage())
            .map_err(|error| CryptoError::Identity(error.to_string()))?;
        let credential = CredentialWithKey {
            credential: BasicCredential::new(device_id.as_bytes().to_vec()).into(),
            signature_key: signer.to_public_vec().into(),
        };
        Ok(Self {
            device_id: device_id.to_owned(),
            provider,
            signer,
            credential,
        })
    }

    /// The device identifier carried inside this device's credential.
    #[must_use]
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Produce a key package another member uses to invite this device.
    ///
    /// The bytes are public. They are safe to hand to the relay, which cannot
    /// use them to read workspace content.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::KeyPackage`] when the bundle cannot be built or
    /// serialized.
    pub fn publish_key_package(&self) -> Result<Vec<u8>> {
        let bundle: KeyPackageBundle = KeyPackage::builder()
            .build(
                CIPHERSUITE,
                &self.provider,
                &self.signer,
                self.credential.clone(),
            )
            .map_err(|error| CryptoError::KeyPackage(error.to_string()))?;
        MlsMessageOut::from(bundle.key_package().clone())
            .tls_serialize_detached()
            .map_err(|error| CryptoError::KeyPackage(error.to_string()))
    }

    /// Rebuild a device identity from previously exported storage.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Identity`] when the storage does not contain the
    /// signature key pair this device was created with.
    pub(crate) fn from_storage(
        device_id: &str,
        provider: LeaveProvider,
        signature_public_key: &[u8],
    ) -> Result<Self> {
        let signer = SignatureKeyPair::read(
            provider.storage(),
            signature_public_key,
            CIPHERSUITE.signature_algorithm(),
        )
        .ok_or_else(|| CryptoError::Identity("stored signing key is missing".into()))?;
        let credential = CredentialWithKey {
            credential: BasicCredential::new(device_id.as_bytes().to_vec()).into(),
            signature_key: signer.to_public_vec().into(),
        };
        Ok(Self {
            device_id: device_id.to_owned(),
            provider,
            signer,
            credential,
        })
    }

    /// The public half of this device's signing key.
    #[must_use]
    pub fn signature_public_key(&self) -> Vec<u8> {
        self.signer.to_public_vec()
    }

    pub(crate) fn provider(&self) -> &LeaveProvider {
        &self.provider
    }

    pub(crate) fn signer(&self) -> &SignatureKeyPair {
        &self.signer
    }

    pub(crate) fn credential(&self) -> CredentialWithKey {
        self.credential.clone()
    }
}

/// Read a published key package and check it before it reaches a group.
///
/// # Errors
///
/// Returns [`CryptoError::KeyPackage`] when the bytes are not a key package
/// for the workspace ciphersuite.
pub(crate) fn parse_key_package(bytes: &[u8], provider: &LeaveProvider) -> Result<KeyPackage> {
    let message = MlsMessageIn::tls_deserialize_exact(bytes)
        .map_err(|error| CryptoError::KeyPackage(error.to_string()))?;
    let MlsMessageBodyIn::KeyPackage(key_package) = message.extract() else {
        return Err(CryptoError::KeyPackage(
            "the invitation did not contain a key package".into(),
        ));
    };
    let key_package: KeyPackage = key_package
        .validate(provider.crypto(), ProtocolVersion::Mls10)
        .map_err(|error| CryptoError::KeyPackage(error.to_string()))?;
    if key_package.ciphersuite() != CIPHERSUITE {
        return Err(CryptoError::KeyPackage(
            "the device offered an unsupported ciphersuite".into(),
        ));
    }
    Ok(key_package)
}

/// Read the device identifier out of a basic credential.
pub(crate) fn identity_of(credential: &openmls::prelude::Credential) -> Result<String> {
    let basic = BasicCredential::try_from(credential.clone())
        .map_err(|_| CryptoError::MalformedIdentity)?;
    String::from_utf8(basic.identity().to_vec()).map_err(|_| CryptoError::MalformedIdentity)
}

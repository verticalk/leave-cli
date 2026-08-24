//! The MLS provider Leave uses, with storage it can save and reload.
//!
//! `OpenMlsRustCrypto` builds its storage internally and offers no way to
//! restore one, so a host restart would lose every workspace group and force
//! every phone to pair again. This composes the same `RustCrypto` backend with a
//! storage handle Leave controls.

use crate::{
    codec::{put_bytes, put_count, take_bytes, take_count},
    error::{CryptoError, Result},
};
use openmls_memory_storage::MemoryStorage;
use openmls_rust_crypto::RustCrypto;
use openmls_traits::OpenMlsProvider;

/// A provider whose key storage can be exported and restored.
#[derive(Default)]
pub(crate) struct LeaveProvider {
    crypto: RustCrypto,
    storage: MemoryStorage,
}

impl LeaveProvider {
    /// Rebuild a provider from bytes produced by [`Self::export`].
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Group`] when the bytes are not valid storage.
    pub(crate) fn restore(cursor: &mut &[u8]) -> Result<Self> {
        let storage = MemoryStorage::default();
        {
            let mut values = storage
                .values
                .write()
                .map_err(|_| CryptoError::Group("saved session storage is poisoned".into()))?;
            let count = take_count(cursor)?;
            for _ in 0..count {
                let key = take_bytes(cursor)?.to_vec();
                let value = take_bytes(cursor)?.to_vec();
                values.insert(key, value);
            }
        }
        Ok(Self {
            crypto: RustCrypto::default(),
            storage,
        })
    }

    /// Serialize every stored key and group secret.
    ///
    /// The result is secret. It carries this device's signing key and the
    /// group's ratchet secrets, so a caller must protect it at least as well
    /// as the workspace content it unlocks.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Encode`] when storage cannot be serialized.
    pub(crate) fn export(&self, buffer: &mut Vec<u8>) -> Result<()> {
        let values = self
            .storage
            .values
            .read()
            .map_err(|_| CryptoError::Encode("session storage is poisoned".into()))?;
        put_count(buffer, values.len());
        // A stable order keeps two exports of one unchanged session identical.
        let mut entries: Vec<_> = values.iter().collect();
        entries.sort_by(|left, right| left.0.cmp(right.0));
        for (key, value) in entries {
            put_bytes(buffer, key);
            put_bytes(buffer, value);
        }
        Ok(())
    }
}

impl OpenMlsProvider for LeaveProvider {
    type CryptoProvider = RustCrypto;
    type RandProvider = RustCrypto;
    type StorageProvider = MemoryStorage;

    fn storage(&self) -> &Self::StorageProvider {
        &self.storage
    }

    fn crypto(&self) -> &Self::CryptoProvider {
        &self.crypto
    }

    fn rand(&self) -> &Self::RandProvider {
        &self.crypto
    }
}

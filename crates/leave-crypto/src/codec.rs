//! A small length-prefixed encoding for saved session state.
//!
//! `OpenMLS` only offers storage serialization behind its test-utility feature,
//! and its persistence helper writes key material into the system temp
//! directory. Leave encodes the state itself so the bytes stay under the
//! caller's control and the format stays stable across upgrades.

use crate::error::{CryptoError, Result};

/// Marks the encoding so a future change can be detected rather than guessed.
pub(crate) const MAGIC: &[u8; 8] = b"LEAVEMLS";
/// Version of the state encoding.
pub(crate) const VERSION: u8 = 1;

/// Append a length-prefixed byte string.
pub(crate) fn put_bytes(buffer: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).unwrap_or(u32::MAX);
    buffer.extend_from_slice(&length.to_be_bytes());
    buffer.extend_from_slice(value);
}

/// Read a length-prefixed byte string, advancing the cursor.
///
/// # Errors
///
/// Returns [`CryptoError::Group`] when the buffer ends early or declares a
/// length it cannot satisfy.
pub(crate) fn take_bytes<'a>(cursor: &mut &'a [u8]) -> Result<&'a [u8]> {
    let (length, rest) = cursor
        .split_at_checked(4)
        .ok_or_else(|| CryptoError::Group("saved session ended inside a length".into()))?;
    let length: [u8; 4] = length
        .try_into()
        .map_err(|_| CryptoError::Group("saved session has a malformed length".into()))?;
    let length = u32::from_be_bytes(length) as usize;
    let (value, rest) = rest
        .split_at_checked(length)
        .ok_or_else(|| CryptoError::Group("saved session ended inside a value".into()))?;
    *cursor = rest;
    Ok(value)
}

/// Append a count.
pub(crate) fn put_count(buffer: &mut Vec<u8>, count: usize) {
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    buffer.extend_from_slice(&count.to_be_bytes());
}

/// Read a count, advancing the cursor.
///
/// # Errors
///
/// Returns [`CryptoError::Group`] when the buffer ends early.
pub(crate) fn take_count(cursor: &mut &[u8]) -> Result<usize> {
    let (count, rest) = cursor
        .split_at_checked(4)
        .ok_or_else(|| CryptoError::Group("saved session ended inside a count".into()))?;
    let count: [u8; 4] = count
        .try_into()
        .map_err(|_| CryptoError::Group("saved session has a malformed count".into()))?;
    *cursor = rest;
    Ok(u32::from_be_bytes(count) as usize)
}

/// Check and consume the header at the start of a saved state.
///
/// # Errors
///
/// Returns [`CryptoError::Group`] when the bytes are not Leave session state
/// or use an unsupported version.
pub(crate) fn take_header(cursor: &mut &[u8]) -> Result<()> {
    let (header, rest) = cursor
        .split_at_checked(MAGIC.len() + 1)
        .ok_or_else(|| CryptoError::Group("saved session is truncated".into()))?;
    if &header[..MAGIC.len()] != MAGIC {
        return Err(CryptoError::Group("not a Leave session state".into()));
    }
    if header[MAGIC.len()] != VERSION {
        return Err(CryptoError::Group(
            "saved session uses an unsupported state version".into(),
        ));
    }
    *cursor = rest;
    Ok(())
}

/// Write the header at the start of a saved state.
pub(crate) fn put_header(buffer: &mut Vec<u8>) {
    buffer.extend_from_slice(MAGIC);
    buffer.push(VERSION);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_values() -> Result<()> {
        let mut buffer = Vec::new();
        put_header(&mut buffer);
        put_count(&mut buffer, 2);
        put_bytes(&mut buffer, b"first");
        put_bytes(&mut buffer, b"");
        let mut cursor = buffer.as_slice();
        take_header(&mut cursor)?;
        assert_eq!(take_count(&mut cursor)?, 2);
        assert_eq!(take_bytes(&mut cursor)?, b"first");
        assert_eq!(take_bytes(&mut cursor)?, b"");
        assert!(cursor.is_empty());
        Ok(())
    }

    #[test]
    fn rejects_foreign_or_truncated_state() {
        let mut cursor: &[u8] = b"not leave state at all";
        assert!(take_header(&mut cursor).is_err());
        let mut cursor: &[u8] = b"LEAVE";
        assert!(take_header(&mut cursor).is_err());

        let mut buffer = Vec::new();
        put_header(&mut buffer);
        put_bytes(&mut buffer, b"value");
        buffer.truncate(buffer.len() - 2);
        let mut cursor = buffer.as_slice();
        assert!(take_header(&mut cursor).is_ok());
        assert!(take_bytes(&mut cursor).is_err());
    }

    #[test]
    fn rejects_a_future_state_version() {
        let mut buffer = Vec::new();
        put_header(&mut buffer);
        buffer[MAGIC.len()] = VERSION + 1;
        let mut cursor = buffer.as_slice();
        assert!(take_header(&mut cursor).is_err());
    }
}

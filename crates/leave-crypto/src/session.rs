//! The MLS group behind one workspace channel.
//!
//! Every command and event that crosses the relay is an MLS application
//! message inside this group. The relay routes the ciphertext and learns
//! nothing about its contents. The host re-authorizes each decrypted command
//! using the sender identity this layer reports, never a value carried in the
//! payload itself.

use crate::{
    error::{CryptoError, Result},
    identity::{DeviceIdentity, identity_of, parse_key_package},
};
use openmls::{
    group::{MlsGroup, MlsGroupCreateConfig, MlsGroupJoinConfig, StagedWelcome},
    prelude::{
        GroupId, MlsMessageBodyIn, MlsMessageIn, ProcessedMessageContent, ProtocolMessage,
        SenderRatchetConfiguration,
        tls_codec::{Deserialize as TlsDeserialize, Serialize as TlsSerialize},
    },
};

/// Bytes of padding applied to every application message.
///
/// Padding blunts the length side channel the relay would otherwise see on
/// short frames such as approvals.
const PADDING_SIZE: usize = 256;
/// How many out-of-order frames a receiver tolerates within one epoch.
const OUT_OF_ORDER_TOLERANCE: u32 = 32;
/// How far ahead of the current position a receiver will ratchet.
const MAXIMUM_FORWARD_DISTANCE: u32 = 2000;

/// An invitation produced when a device is added to a workspace.
#[derive(Debug, Clone)]
pub struct Invitation {
    /// Commit that existing members apply to move the group forward.
    pub commit: Vec<u8>,
    /// Welcome the invited device uses to join.
    pub welcome: Vec<u8>,
}

/// A decrypted application message and the device that actually sent it.
#[derive(Debug, Clone)]
pub struct OpenedMessage {
    /// Device identifier taken from the authenticated MLS credential.
    pub sender_device_id: String,
    /// Decrypted application payload.
    pub plaintext: Vec<u8>,
}

/// One device's view of a workspace's MLS group.
pub struct WorkspaceSession {
    identity: DeviceIdentity,
    group: MlsGroup,
}

impl WorkspaceSession {
    /// Create a new workspace group with this device as its only member.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Group`] when the group cannot be created.
    pub fn create(identity: DeviceIdentity, workspace_id: &str) -> Result<Self> {
        let group = MlsGroup::new_with_group_id(
            identity.provider(),
            identity.signer(),
            &create_config(),
            GroupId::from_slice(workspace_id.as_bytes()),
            identity.credential(),
        )
        .map_err(|error| CryptoError::Group(error.to_string()))?;
        Ok(Self { identity, group })
    }

    /// Join a workspace from the welcome produced by [`Self::add_device`].
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Group`] when the welcome is malformed or was not
    /// addressed to this device.
    pub fn join(identity: DeviceIdentity, welcome: &[u8]) -> Result<Self> {
        let message = MlsMessageIn::tls_deserialize_exact(welcome)
            .map_err(|error| CryptoError::Group(error.to_string()))?;
        let MlsMessageBodyIn::Welcome(welcome) = message.extract() else {
            return Err(CryptoError::UnexpectedMessage);
        };
        let staged =
            StagedWelcome::new_from_welcome(identity.provider(), &join_config(), welcome, None)
                .map_err(|error| CryptoError::Group(error.to_string()))?;
        let group = staged
            .into_group(identity.provider())
            .map_err(|error| CryptoError::Group(error.to_string()))?;
        Ok(Self { identity, group })
    }

    /// Add a device using the key package it published.
    ///
    /// The commit is merged locally before returning, so the caller must
    /// deliver both parts: the commit to existing members and the welcome to
    /// the invited device.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::KeyPackage`] for an unusable key package and
    /// [`CryptoError::Group`] when the commit cannot be created or merged.
    pub fn add_device(&mut self, key_package: &[u8]) -> Result<Invitation> {
        let key_package = parse_key_package(key_package, self.identity.provider())?;
        let (commit, welcome, _group_info) = self
            .group
            .add_members(
                self.identity.provider(),
                self.identity.signer(),
                core::slice::from_ref(&key_package),
            )
            .map_err(|error| CryptoError::Group(error.to_string()))?;
        self.group
            .merge_pending_commit(self.identity.provider())
            .map_err(|error| CryptoError::Group(error.to_string()))?;
        Ok(Invitation {
            commit: encode(&commit)?,
            welcome: encode(&welcome)?,
        })
    }

    /// Remove a device and rotate the group forward past it.
    ///
    /// After the returned commit is applied, the removed device cannot read
    /// any later message, including ones it can still observe on the relay.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::UnknownDevice`] when no member carries that
    /// identifier, or [`CryptoError::Group`] when the commit fails.
    pub fn remove_device(&mut self, device_id: &str) -> Result<Vec<u8>> {
        let index = self
            .group
            .members()
            .find(|member| {
                identity_of(&member.credential).is_ok_and(|identity| identity == device_id)
            })
            .map(|member| member.index)
            .ok_or(CryptoError::UnknownDevice)?;
        let (commit, _welcome, _group_info) = self
            .group
            .remove_members(self.identity.provider(), self.identity.signer(), &[index])
            .map_err(|error| CryptoError::Group(error.to_string()))?;
        self.group
            .merge_pending_commit(self.identity.provider())
            .map_err(|error| CryptoError::Group(error.to_string()))?;
        encode(&commit)
    }

    /// Apply a commit produced by another member.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::UnexpectedMessage`] when the frame is not a
    /// commit, or [`CryptoError::Group`] when it cannot be applied.
    pub fn apply_commit(&mut self, commit: &[u8]) -> Result<()> {
        let processed = self.process(commit)?;
        match processed.into_content() {
            ProcessedMessageContent::StagedCommitMessage(staged) => {
                self.group
                    .merge_staged_commit(self.identity.provider(), *staged)
                    .map_err(|error| CryptoError::Group(error.to_string()))?;
                Ok(())
            }
            _ => Err(CryptoError::UnexpectedMessage),
        }
    }

    /// Encrypt one application payload for the workspace.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Seal`] when the group cannot produce a frame.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let message = self
            .group
            .create_message(self.identity.provider(), self.identity.signer(), plaintext)
            .map_err(|error| CryptoError::Seal(error.to_string()))?;
        encode(&message)
    }

    /// Decrypt one application frame and report its authenticated sender.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::Open`] when the frame does not decrypt or
    /// authenticate, and [`CryptoError::UnexpectedMessage`] when it decrypts
    /// to something other than an application message.
    pub fn open(&mut self, ciphertext: &[u8]) -> Result<OpenedMessage> {
        let processed = self.process(ciphertext)?;
        let sender_device_id = identity_of(processed.credential())?;
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(message) => Ok(OpenedMessage {
                sender_device_id,
                plaintext: message.into_bytes(),
            }),
            _ => Err(CryptoError::UnexpectedMessage),
        }
    }

    /// The group's current epoch.
    ///
    /// The epoch advances on every membership change, which lets the host
    /// reject frames produced before a device was removed.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.group.epoch().as_u64()
    }

    /// Device identifiers of every current member.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::MalformedIdentity`] when a member credential
    /// does not carry readable text.
    pub fn member_device_ids(&self) -> Result<Vec<String>> {
        self.group
            .members()
            .map(|member| identity_of(&member.credential))
            .collect()
    }

    /// This device's own identifier.
    #[must_use]
    pub fn device_id(&self) -> &str {
        self.identity.device_id()
    }

    fn process(&mut self, bytes: &[u8]) -> Result<openmls::prelude::ProcessedMessage> {
        let message = MlsMessageIn::tls_deserialize_exact(bytes)
            .map_err(|error| CryptoError::Open(error.to_string()))?;
        let protocol: ProtocolMessage = message
            .try_into_protocol_message()
            .map_err(|error| CryptoError::Open(error.to_string()))?;
        self.group
            .process_message(self.identity.provider(), protocol)
            .map_err(|error| CryptoError::Open(error.to_string()))
    }
}

fn create_config() -> MlsGroupCreateConfig {
    MlsGroupCreateConfig::builder()
        .padding_size(PADDING_SIZE)
        .sender_ratchet_configuration(SenderRatchetConfiguration::new(
            OUT_OF_ORDER_TOLERANCE,
            MAXIMUM_FORWARD_DISTANCE,
        ))
        .ciphersuite(crate::identity::CIPHERSUITE)
        // Devices join from a welcome alone, without fetching the tree
        // separately from the relay.
        .use_ratchet_tree_extension(true)
        .build()
}

fn join_config() -> MlsGroupJoinConfig {
    MlsGroupJoinConfig::builder()
        .padding_size(PADDING_SIZE)
        .sender_ratchet_configuration(SenderRatchetConfiguration::new(
            OUT_OF_ORDER_TOLERANCE,
            MAXIMUM_FORWARD_DISTANCE,
        ))
        .use_ratchet_tree_extension(true)
        .build()
}

fn encode(message: &impl TlsSerialize) -> Result<Vec<u8>> {
    message
        .tls_serialize_detached()
        .map_err(|error| CryptoError::Encode(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a workspace owned by `owner`, with every listed device joined.
    fn workspace(
        owner: &str,
        joiners: &[&str],
    ) -> Result<(WorkspaceSession, Vec<WorkspaceSession>)> {
        let mut host = WorkspaceSession::create(DeviceIdentity::generate(owner)?, "workspace-1")?;
        let mut joined = Vec::new();
        for name in joiners {
            let device = DeviceIdentity::generate(name)?;
            let invitation = host.add_device(&device.publish_key_package()?)?;
            for existing in &mut joined {
                WorkspaceSession::apply_commit(existing, &invitation.commit)?;
            }
            joined.push(WorkspaceSession::join(device, &invitation.welcome)?);
        }
        Ok((host, joined))
    }

    #[test]
    fn a_joined_phone_reads_workspace_traffic() -> Result<()> {
        let (mut host, mut phones) = workspace("host", &["phone"])?;
        let phone = &mut phones[0];

        let frame = host.seal(b"session started")?;
        let opened = phone.open(&frame)?;
        assert_eq!(opened.plaintext, b"session started");
        assert_eq!(opened.sender_device_id, "host");

        let reply = phone.seal(b"approve once")?;
        let opened = host.open(&reply)?;
        assert_eq!(opened.plaintext, b"approve once");
        assert_eq!(opened.sender_device_id, "phone");
        Ok(())
    }

    #[test]
    fn the_relay_never_sees_the_plaintext() -> Result<()> {
        let (mut host, _phones) = workspace("host", &["phone"])?;
        let secret = b"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5 deploy key";
        let frame = host.seal(secret)?;
        assert!(
            !frame.windows(secret.len()).any(|window| window == secret),
            "the sealed frame must not contain the plaintext"
        );
        Ok(())
    }

    #[test]
    fn short_frames_are_padded_to_hide_their_length() -> Result<()> {
        let (mut host, _phones) = workspace("host", &["phone"])?;
        let approve = host.seal(b"y")?;
        let deny = host.seal(b"no, cancel that")?;
        assert_eq!(
            approve.len(),
            deny.len(),
            "short decisions must not be distinguishable by frame length"
        );
        Ok(())
    }

    #[test]
    fn every_device_in_the_workspace_shares_one_group() -> Result<()> {
        let (mut host, mut devices) = workspace("host", &["phone", "tablet"])?;
        let frame = host.seal(b"fan out")?;
        for device in &mut devices {
            assert_eq!(device.open(&frame)?.plaintext, b"fan out");
        }
        let mut names = host.member_device_ids()?;
        names.sort();
        assert_eq!(names, ["host", "phone", "tablet"]);
        Ok(())
    }

    #[test]
    fn a_removed_phone_cannot_read_later_messages() -> Result<()> {
        let (mut host, mut devices) = workspace("host", &["phone", "tablet"])?;
        let before = host.epoch();

        let commit = host.remove_device("phone")?;
        devices[1].apply_commit(&commit)?;
        assert!(
            host.epoch() > before,
            "removal must rotate the group forward"
        );

        let frame = host.seal(b"work after revocation")?;
        assert_eq!(devices[1].open(&frame)?.plaintext, b"work after revocation");
        assert!(
            devices[0].open(&frame).is_err(),
            "a revoked device must not read work committed after its removal"
        );
        Ok(())
    }

    #[test]
    fn removing_an_unknown_device_is_refused() -> Result<()> {
        let (mut host, _devices) = workspace("host", &["phone"])?;
        assert!(matches!(
            host.remove_device("laptop"),
            Err(CryptoError::UnknownDevice)
        ));
        Ok(())
    }

    #[test]
    fn a_tampered_frame_is_rejected() -> Result<()> {
        let (mut host, mut phones) = workspace("host", &["phone"])?;
        let mut frame = host.seal(b"delete everything")?;
        let last = frame.len() - 1;
        frame[last] ^= 0xff;
        assert!(phones[0].open(&frame).is_err());
        Ok(())
    }

    #[test]
    fn a_replayed_frame_is_rejected() -> Result<()> {
        let (mut host, mut phones) = workspace("host", &["phone"])?;
        let frame = host.seal(b"approve once")?;
        assert_eq!(phones[0].open(&frame)?.plaintext, b"approve once");
        assert!(
            phones[0].open(&frame).is_err(),
            "an approval must not be replayable"
        );
        Ok(())
    }

    #[test]
    fn a_commit_is_not_accepted_as_an_application_message() -> Result<()> {
        let (mut host, mut phones) = workspace("host", &["phone"])?;
        let device = DeviceIdentity::generate("tablet")?;
        let invitation = host.add_device(&device.publish_key_package()?)?;
        assert!(matches!(
            phones[0].open(&invitation.commit),
            Err(CryptoError::UnexpectedMessage)
        ));
        Ok(())
    }

    #[test]
    fn junk_key_packages_are_refused() -> Result<()> {
        let (mut host, _phones) = workspace("host", &[])?;
        assert!(matches!(
            host.add_device(b"not a key package"),
            Err(CryptoError::KeyPackage(_))
        ));
        Ok(())
    }

    #[test]
    fn a_welcome_for_someone_else_does_not_open_the_workspace() -> Result<()> {
        let mut host = WorkspaceSession::create(DeviceIdentity::generate("host")?, "workspace-1")?;
        let phone = DeviceIdentity::generate("phone")?;
        let invitation = host.add_device(&phone.publish_key_package()?)?;
        let attacker = DeviceIdentity::generate("attacker")?;
        assert!(WorkspaceSession::join(attacker, &invitation.welcome).is_err());
        Ok(())
    }
}

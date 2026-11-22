use crate::*;
use curve25519_dalek::{edwards::CompressedEdwardsY, scalar::Scalar, traits::IsIdentity};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

pub fn derive_public_key(seed: &[u8; 32]) -> SignerKey {
    SignerKey(SigningKey::from_bytes(seed).verifying_key().to_bytes())
}

pub fn validate_signer_key(key: &SignerKey) -> Result<(), ProtocolError> {
    validate_point(&key.0, true)
}
fn validate_point(bytes: &[u8; 32], nonidentity: bool) -> Result<(), ProtocolError> {
    let point = CompressedEdwardsY(*bytes)
        .decompress()
        .ok_or(ProtocolError::InvalidSignature)?;
    if point.compress().to_bytes() != *bytes
        || !point.is_torsion_free()
        || (nonidentity && point.is_identity())
    {
        return Err(ProtocolError::InvalidSignature);
    }
    Ok(())
}

pub fn canonical_unsigned(draft: &UnsignedEvent) -> Result<Vec<u8>, ProtocolError> {
    if draft.version != 1 {
        return Err(ProtocolError::UnsupportedVersion);
    }
    if draft.created_at > MAX_SAFE_INTEGER || draft.kind > MAX_SAFE_INTEGER {
        return Err(ProtocolError::InvalidEvent);
    }
    // Also validate recursively when the caller constructs Values programmatically.
    let bytes = serde_json::to_vec(draft).map_err(|_| ProtocolError::InvalidJson)?;
    parse_json(
        &bytes,
        ParseLimits {
            max_bytes: bytes.len(),
            max_depth: 128,
        },
    )?;
    serde_jcs::to_vec(draft).map_err(|_| ProtocolError::InvalidJson)
}
pub fn event_id(draft: &UnsignedEvent) -> Result<EventId, ProtocolError> {
    Ok(EventId(Sha256::digest(canonical_unsigned(draft)?).into()))
}
pub fn sign_event(draft: &UnsignedEvent, seed: &[u8; 32]) -> Result<VerifiedEvent, ProtocolError> {
    if derive_public_key(seed) != draft.signer {
        return Err(ProtocolError::InvalidSignature);
    }
    let id = event_id(draft)?;
    let signature = SignatureBytes(SigningKey::from_bytes(seed).sign(&id.0).to_bytes());
    Ok(VerifiedEvent(Event {
        version: draft.version,
        id,
        signer: draft.signer,
        created_at: draft.created_at,
        kind: draft.kind,
        header: draft.header.clone(),
        content: draft.content.clone(),
        signature,
    }))
}
pub fn parse_and_verify(bytes: &[u8], limits: ParseLimits) -> Result<VerifiedEvent, ProtocolError> {
    let event = parse_event(bytes, limits)?;
    if event_id(&event.unsigned())? != event.id {
        return Err(ProtocolError::InvalidId);
    }
    validate_signer_key(&event.signer)?;
    let r: [u8; 32] = event.signature.0[..32]
        .try_into()
        .map_err(|_| ProtocolError::InvalidSignature)?;
    validate_point(&r, false)?;
    let s: [u8; 32] = event.signature.0[32..]
        .try_into()
        .map_err(|_| ProtocolError::InvalidSignature)?;
    if !bool::from(Scalar::from_canonical_bytes(s).is_some()) {
        return Err(ProtocolError::InvalidSignature);
    }
    // MIP-02 permits canonical identity R. verify_strict additionally rejects it.
    let key =
        VerifyingKey::from_bytes(&event.signer.0).map_err(|_| ProtocolError::InvalidSignature)?;
    let signature = ed25519_dalek::Signature::from_bytes(&event.signature.0);
    key.verify(&event.id.0, &signature)
        .map_err(|_| ProtocolError::InvalidSignature)?;
    Ok(VerifiedEvent(event))
}

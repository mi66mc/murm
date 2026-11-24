# Plan 01 — Shared Protocol Library

> Historical implementation plan. The original execution guidance was to use
> superpowers:executing-plans or superpowers:subagent-driven-development, one task at a time.

**Goal:** Implement the deterministic rules shared by the relay and client.
**Architecture:** The murm-protocol crate is independent of networking, SQLite, the vault, and the terminal.
**Proposed stack:** Serde, serde_json, serde_jcs, SHA-256, ed25519-dalek,
curve25519-dalek for point validation, and zeroizing types where needed.
**Spec:** [Design](../design.md) and MIPs 01–05 at the
[reference commit](https://github.com/mi66mc/mips/tree/c3d601848cd05c41b3726a9dc2631239656da8e7).

## Global constraints

Rust edition 2024, AGPL-3.0-only, and the three existing crates.
An event has exactly eight fields; hash the six-field JCS object and apply
pure Ed25519 to the 32-byte digest. Safe integers extend up to 9007199254740991.
Do not add fields for accounts, local profiles, passwords, devices, or backup.
Local relay settings do not change cryptographic validity.

Use stable dependency versions compatible with the toolchain. The resolved
versions are pinned in Cargo.lock.

## Review focus

1. Duplicate JSON properties, including in unknown objects.
2. UTF-16/JCS ordering, numbers, negative zero, and Unicode without normalization.
3. Accidentally signing hexadecimal text instead of the digest.
4. Differences between the MIPs' Ed25519 point rules and library defaults.
5. Resurfacing a withdrawn article by filtering revisions before selecting the preferred one.

## Files and contracts

Create the following in crates/murm-protocol/src:
event.rs, json.rs, crypto.rs, publication.rs, filter.rs, api.rs, and error.rs.
Modify lib.rs and Cargo.toml; declare shared dependencies in the root Cargo.toml.

Public types and functions use the names from the design. EventId and SignerKey
are 32-byte wrappers encoded as lowercase hex in JSON. SignatureBytes contains
64 bytes. Event holds all eight fields; header and content preserve JsonValue.
UnsignedEvent contains only the six signable fields. VerifiedEvent wraps Event
after structural and cryptographic validation; arbitrary public construction is
not allowed. PublicationMeta holds the logical key, revision, ID, and status.

ParseLimits takes max_bytes and max_depth from the caller. ProtocolError
distinguishes invalid JSON, shape, version, ID, signature, kind, filter, and limit
errors. Do not load admission limits from global configuration in this crate.

### Task 1: Parsing and representation

**Files:** src/event.rs, src/json.rs, src/error.rs, tests/event_input.rs.

**Provides:** `parse_event(bytes: &[u8], limits: ParseLimits) -> Result<Event, ProtocolError>`
and the types above. This does not yet return VerifiedEvent.

- [ ] Write tests duplicate_nested_member_rejected, unknown_outer_field_rejected,
  missing_field_rejected, unknown_nested_field_preserved, invalid_unicode_rejected,
  unsafe_integer_rejected, and byte_and_depth_limit_rejected.
  Reject duplicate members in header and content before serde_json::Value
  can discard an occurrence. An extra top-level field is an error, not a field to drop.
- [ ] Run cargo test -p murm-protocol --test event_input; expect failure because
  the parser does not exist yet.
- [ ] Implement a parser with a visitor that detects duplicate members in every
  object and validates numbers and Unicode before constructing the types. Do not
  rely solely on Value deserialized by a parser that overwrites duplicate keys.
- [ ] Run the same tests and check that they pass and unknown fields are preserved.
- [ ] Commit: feat(protocol): add strict event parsing.

### Task 2: ID, signing, and verification

**Files:** src/crypto.rs, src/lib.rs, tests/event_crypto.rs.
**Uses:** Event, UnsignedEvent, ParseLimits, ProtocolError.
**Provides:** `canonical_unsigned(&UnsignedEvent) -> Result<Vec<u8>, ProtocolError>`,
`event_id(&UnsignedEvent) -> Result<EventId, ProtocolError>`, sign_event and
parse_and_verify from the design, and `derive_public_key(seed: &[u8; 32]) -> SignerKey`.

- [ ] Test mip02_known_event with the seed, payload, and signature published in
  MIP-02. The key assertion is:

  ```rust
  assert_eq!(id.to_string(),
      "945708370d265a3825922c890b2ac7f09fb0d5a68fa5e12e9b768ec0e3790791");
  ```

  Also check the entire signature and the published canonical bytes.
  Add raw_digest_not_hex, each_signed_field_changes_id,
  formatting_and_object_order_preserve_id, array_order_changes_id,
  unicode_not_normalized, and negative_zero_uses_jcs_zero.
- [ ] Run cargo test -p murm-protocol --test event_crypto and observe the failures.
- [ ] Implement with serde_jcs and established primitives. The payload is an object.
  Check that public-key and R encodings are canonical and the points are torsion-free.
  The public key must not be the identity. S must be canonical and less than L.
  Then use the library's verification equation. Do not substitute verify_strict
  without checking the differences: the identity for R belongs to the subgroup
  but the library's strict default may reject it. The MIP is authoritative.
  Do not enable batch verification with semantics different from individual verification.
- [ ] Add noncanonical_point_rejected, torsion_point_rejected,
  identity_public_key_rejected, scalar_out_of_range_rejected, and a valid identity-R
  test using an independent algebraic test construction. Do not implement production
  cryptographic primitives. Document any library conflict as an incompatibility
  to fix; do not silently change the MIP.
- [ ] Run cargo test -p murm-protocol --test event_crypto and check that all tests pass.
- [ ] Commit: feat(protocol): implement canonical IDs and event verification.

### Task 3: Publications and revision preference

**Files:** src/publication.rs, tests/publications.rs.
**Uses:** VerifiedEvent and binary EventId comparison.
**Provides:** validate_publication and compare_revisions from the design.

- [ ] Test identifier_bounds, metadata_bounds, withdrawn_requires_null,
  published_requires_string, missing_predecessor_allowed,
  greater_revision_then_greater_id, another_signer_is_another_publication,
  unknown_metadata_preserved, and maximum_revision_does_not_wrap.
- [ ] Run cargo test -p murm-protocol --test publications; expect failures.
- [ ] Implement the MIP-04 limits exactly: a 1–128 character ASCII publication ID;
  title up to 200, summary up to 1000, up to 20 unique topic strings of 1–64 Unicode
  scalar values each, and language of length 1–64. Use exact comparison. Do not
  render Pandoc. compare_revisions rejects different logical keys.
- [ ] Run the tests and check kind 1 and safe positive revision numbers.
- [ ] Commit: feat(protocol): validate publication revisions.

### Task 4: Filters and HTTP contracts

**Files:** src/filter.rs, src/api.rs, tests/query_contract.rs.
**Provides:** Filter, the API types from the design, validate_query and
validate_sync with RelayInfo and capacity limits, matches_filter, ProblemCode, and Problem.

- [ ] Test and_inside_or_between, deduplicate_by_id, typed_equality,
  missing_vs_null, contains_is_array_membership, since_inclusive_until_exclusive,
  unknown_request_member_rejected, header_requires_explicit_kinds,
  unsupported_path_rejected, and current_requires_supported_kinds.
  Deserialize the complete MIP-03 /info example. Check the relationship between
  accepted, validated, and current kinds, default policies and overrides, and error codes.
- [ ] Run cargo test -p murm-protocol --test query_contract; expect failures.
- [ ] Implement without generating SQL. The relay validates capacity before
  filtering. matches_filter never uses the network. A cursor is an opaque string.
  Keep QueryResponse(next_cursor) distinct from SyncResponse(cursor, has_more).
  Responses accept extra metadata; requests reject unknown members.
- [ ] Run the tests. Check that a valid signature does not imply editorial truth.
- [ ] Commit: feat(protocol): add query and relay API contracts.

## Exit criteria

Run cargo fmt --all -- --check and cargo test -p murm-protocol.
This crate does not depend on Axum, Rusqlite, or Ratatui. Tests use inline
examples without a new schemas directory, test-vectors directory, or generators.
Build the storage layers on these contracts.

# Plan 03 — Client, Profiles, Vault, and Local SQLite

> Historical implementation plan. The original execution guidance was to use
> superpowers:executing-plans or superpowers:subagent-driven-development, one task at a time.

**Goal:** Make murm-cli a persistent client with generated and imported profiles,
commands, and services reusable by the TUI.
**Architecture:** Add lib.rs to the existing crate. Profiles, the vault, storage,
the outbox, and transport are services with no widget dependencies.
**Proposed stack:** Rusqlite with bundled SQLite, Tokio, Reqwest, Clap, platform
data directories, Argon2id, XChaCha20-Poly1305, zeroize, the system RNG, and a process lock.
**Spec:** [Design](../design.md), [protocol](01-protocol.md), and [relay](02-relay.md).

## Global constraints

Do not change protocol identity. A profile is a local record of a public key.
The client and relay use separate SQLite files. Store seeds only as ciphertext.
Do not implement DMs, standardized backup, or identity rotation or delegation.
Publications use Pandoc Markdown source. No secret goes to the relay.
Keep AGPL-3.0-only and the existing crates; do not create a fourth client crate.

## Review focus

1. An imported public key mismatches the private key, or a duplicate public key creates another profile.
2. A password or seed appears in SQLite, WAL, arguments, logs, or an error.
3. An incorrect or corrupt vault overwrites data and loses access to profiles.
4. A network error after POST creates a new signature or revision on retry.
5. A batch or profile switch advances a cursor or applies a response in the wrong context.

## Files and interfaces

Create src/lib.rs, app.rs, error.rs, profiles.rs, vault.rs, transport.rs,
articles.rs, sync.rs, outbox.rs, commands.rs, storage/mod.rs, and
storage/migrations.rs. main.rs remains the binary entry point.

Client types:

- ProfileId: local UUID with no protocol meaning.
- ProfileMode: ReadOnly or Signing. ProfileSummary contains the ID, name, SignerKey,
  mode, locked state, and relays, but never a seed.
- ProfileInput: Generate, ImportSecret(seed, optional public_key), ImportPublic.
  Secret material is a zeroizing wrapper without Debug or Serialize.
- DraftId and Draft: author/ProfileId, publication, revision, source, metadata,
  a local concurrency version, and an optional queued event.
- ClientError: VaultNotInitialized, Locked, ReadOnly, InvalidKey, KeyMismatch,
  WrongPassword, CorruptVault, NotFound, Conflict, Protocol, Network, Database.
- VaultActionId: a token tied to the vault generation, with no secrets.
  VaultStatus reports only whether the vault exists and is unlocked.
- ClientService: owns the services and holds database-worker and vault handles.
  Public async operations:
  - `list_profiles() -> Result<Vec<ProfileSummary>, ClientError>`;
  - `create_profile(name, ProfileInput) -> Result<ProfileSummary, ClientError>`;
  - `select_profile(ProfileId) -> Result<(), ClientError>`;
  - `begin_vault_action() -> VaultActionId`;
  - `initialize_vault(password: SecretString, action: VaultActionId) -> Result<VaultStatus, ClientError>`;
  - `unlock(password: SecretString, action: VaultActionId) -> Result<VaultStatus, ClientError>`;
  - `change_password(current: SecretString, new: SecretString, action: VaultActionId) -> Result<(), ClientError>`;
  - `cancel_vault_action(VaultActionId)`; `lock()`;
  - `save_draft(ProfileId, Draft) -> Result<DraftId, ClientError>`;
  - `publish(ProfileId, DraftId) -> Result<EventId, ClientError>`;
  - `sync_profile(ProfileId, RelayUrl) -> Result<SyncSummary, ClientError>`.
- RelayTransport implements info, submit, fetch, query, and sync using Plan 01
  types. It receives the URL and network limits, never a private profile or password.
- The service emits ClientEvent with task IDs, ProfileId, and UI generation.
  Views have no access to ClientStore or VaultKey.

### Task 1: Local database and read-only profiles

**Files:** storage/*, profiles.rs, error.rs, tests/profiles.rs.

- [ ] Test create_read_only_and_restart, public_key_unique, rename_keeps_identity,
  independent_client_databases, migration_failure_preserves_database, and
  second_instance_is_refused.
- [ ] Run cargo test -p murm-cli --test profiles; expect failures.
- [ ] Create tables client_meta, profiles, vault, profile_secrets,
  profile_relays, drafts, verified_events, publication_heads, sync_cursors,
  outbox, and outbox_destinations. profiles.public_key is UNIQUE; secrets
  exist only for Signing. Profiles can share public events in the cache, but
  drafts, follow cursors, preferences, and outbox items have explicit profile scope.
- [ ] Apply the design's migrations, WAL, foreign keys, and dedicated worker.
  The CLI and TUI resolve the data directory in the same way; --data-dir
  overrides the default. The process lock covers writes, with a clear error
  for a second instance.
- [ ] Run the tests and commit: feat(client): persist local profiles.

### Task 2: Vault and identity import

**Files:** vault.rs, profiles.rs, tests/vault.rs.
**Uses:** derive_public_key, SignerKey, and the Plan 01 seed format.

- [ ] Test generated_profile_can_sign_after_restart, import_secret_derives_public,
  mismatched_pair_rolls_back, upgrade_read_only_preserves_state,
  duplicate_import_is_idempotent, wrong_password_does_not_mutate,
  ciphertext_tampering_fails, changing_password_is_atomic, and lock_blocks_signing.
- [ ] Run cargo test -p murm-cli --test vault; expect failures.
- [ ] Implement the vault as designed: 32-byte master key, Argon2id with
  65536 KiB/3/1, a 16-byte salt, and XChaCha20-Poly1305 with a 24-byte nonce.
  Associated data identifies the version, context, and public key. Copying
  ciphertext to another profile must not yield an accepted seed.
- [ ] Create or upgrade the profile and store the encrypted seed in one transaction.
  Validate or derive the public key before writing. Upgrading requires an
  unlocked vault. The vault worker gives widgets and transport signatures, not seeds.
- [ ] Check vault parameters before running the KDF. Clear transient password and
  seed buffers. Do not derive Debug for secret types. Do not claim guaranteed
  physical erasure from memory, old WAL, or backups. Drafts are not encrypted.
- [ ] Add plaintext_seed_absent_from_db_and_wal using a known test seed and a
  scan of the database and WAL bytes. Add secret_not_in_error_or_debug and
  malformed_vault_parameters_rejected.
- [ ] Add cancel_during_kdf_never_unlocks_later and
  cancel_after_initialization_commit_leaves_locked_vault. The service coordinates
  the generation and token with secret installation under mutual exclusion.
  The KDF runs outside the lock. Lock or cancel invalidates an attempt and
  removes any secret it installed. Creating a Signing profile requires an
  unlocked vault; if no vault exists, return VaultNotInitialized, an explicit
  ClientError variant. The app checks VaultStatus and calls initialize_vault first.
- [ ] Run the tests and commit: feat(client): encrypt signing profiles locally.

### Task 3: Transport, cache, and sync

**Files:** transport.rs, sync.rs, tests/client_sync.rs.
**Uses:** Shared APIs, ClientStore, and the Plan 02 relay.

- [ ] Test verify_before_cache, sync_cursor_commits_with_events,
  invalid_batch_leaves_cursor_unchanged, replay_deduplicates, late_event_is_visible,
  expired_cursor_rescans, retry_after_respected,
  http_alias_fallback_only_for_method_failure, and network_timeout_is_bounded.
- [ ] Run cargo test -p murm-cli --test client_sync; expect failures.
- [ ] Configure URLs per profile, allowing HTTP on loopback or HTTPS, without
  credentials in the URL. Default to configurable limits of 5 seconds to connect
  and 20 seconds per operation. Do not follow redirects automatically. Fetch
  /info and validate its claims without treating them as proof of honesty.
  Negotiate QUERY and its fallback according to MIP-03.
- [ ] Validate each event with the library before storage and apply local
  revision preference. Commit accepted events, heads, and the final cursor in
  the same transaction. The decoder preserves event bytes or uses the shared
  strict visitor before constructing a Value. It must not drop duplicate response
  fields and then verify a changed event. Semantic failures for a kind advertised
  as validated follow the invalid-batch rule. Storage failure cannot advance the cursor.
- [ ] On cursor expiry, restart from retained history and deduplicate by ID.
  Use stable filters that do not exclude withdrawals. Each subscription, peer,
  filter, and profile has explicit scope. Do not reuse a cursor across different
  filter signatures.
- [ ] Run the tests and commit: feat(client): add verified relay synchronization.

### Task 4: Articles and outbox

**Files:** articles.rs, outbox.rs, tests/publishing.rs.
**Provides:** Drafts and publication operations for the TUI.

- [ ] Test read_only_cannot_publish, locked_cannot_sign, draft_survives_restart,
  publish_uses_selected_profile, duplicate_publish_reuses_queued_event,
  timeout_retries_same_id, editing_uses_max_known_revision, withdraw_and_restore,
  stale_revision_conflict_is_visible, and switching_profile_keeps_outbox_owner.
- [ ] Run cargo test -p murm-cli --test publishing; expect failures.
- [ ] Generate a random, stable publication identifier for the first draft.
  A name or title is not its identity. Editing uses the highest known revision
  plus one, without overflow. Never claim it is globally the highest. A
  withdrawal is an event with null content.
- [ ] Confirm the content and ProfileId before signing. Persist the signed event
  and destinations before contacting the network. The outbox keeps the same
  bytes and ID on retry. Publishing the same queued snapshot again returns the
  same ID; it does not create a revision merely because the clock changed.
- [ ] Store each destination's result: pending, stored with retain_until,
  refused, or retry_at. Do not describe stored as delivered, read, or replicated.
  If the network succeeds and local storage fails, retry with the same event.
  Client retries are user-triggered and subject to backoff.
- [ ] A client that knows about a withdrawal does not show an older revision as
  current, even if another relay returns it. Historical cache and current view are separate.
- [ ] Run the tests and commit: feat(client): publish revision events from durable outbox.

### Task 5: Commands and TUI service API

**Files:** app.rs, commands.rs, main.rs, tests/commands.rs.
**Uses:** All the preceding services.

- [ ] Test profile_commands_match_service, public_import_never_unlocks,
  secret_input_not_in_argv, cli_uses_same_database_as_tui, and headless_never_hangs.
- [ ] Run cargo test -p murm-cli --test commands; expect failures.
- [ ] Implement profile create/import/list/use/rename/remove, vault init/passwd,
  relay add/list/remove, article draft/edit/publish/history/withdraw/restore,
  fetch, query, sync, and outbox retry. --public-key accepts a public key as an
  argument. Supply a private key only via --secret-file, --secret-stdin, or a
  masked form. Derive and check the public key.
- [ ] Read the password from a masked terminal prompt or --password-file for
  automation, never a --password value or an environment variable. If stdin
  contains the seed, prompt for the password on the controlling terminal. With
  no terminal, require a password file or fail immediately. Do not read two
  secrets ambiguously from the same stream.
- [ ] Export ClientService, ClientEvent, and DTOs without VaultKey. Both the
  commands and TUI use the same services for publication and local changes.
  Removing a profile requires explicit confirmation or --yes, changes no
  remote data, and does not promise to destroy existing copies of the key.
- [ ] Run the tests and commit: feat(client): expose profile and article commands.

## Exit criteria

Run cargo test -p murm-cli --lib and the profiles, vault, client_sync,
publishing, and commands tests. Check cargo tree -p murm-cli for the absence
of a murm-relay dependency. Profiles and drafts survive a restart, and the
password does not create a protocol account. This plan covers services and
commands; Plan 04 covers screens and opening the TUI without a subcommand.

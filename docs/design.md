# Murm in Rust: First Prototype Design

This is the approved design for the first prototype. See the [README](../README.md) for implementation status and current limits.

## Goal and design decisions

Build a persistent relay and an installable client with commands and a TUI,
sharing one protocol implementation. The client stores profiles, keys, drafts,
and synced data in SQLite; the relay has its own SQLite database.

The user chose Rust, a Cargo workspace, three crates, AGPL-3.0-only, a client TUI,
persistent profiles, and key generation and import. The public key alone is the
protocol identity. A local profile is neither an account on the relay nor a
profile event of some as-yet undefined kind.

The design uses read-only profiles for standalone public keys, an encrypted vault
with a local password, Ratatui/Crossterm, rusqlite with bundled SQLite, Axum/Tokio,
and Reqwest for HTTP. The TUI edits drafts in place using ratatui-textarea;
an external editor remains optional for command-line use.

The protocol reference is the MIPs repository at commit
[c3d6018](https://github.com/mi66mc/mips/tree/c3d601848cd05c41b3726a9dc2631239656da8e7).
The earlier TypeScript suggestion in MIP-05 is non-normative; the user chose Rust
later. The plans do not change the protocol to accommodate a library.

## Approaches considered

1. Store client profiles and encrypted keys in SQLite. This is portable, works
   offline, and does not require a system secret manager. This is the chosen approach.
2. Use SQLite for public data only and put keys in a system keychain. This needs
   extra per-platform integration and could be a future backend.
3. Require an external signer. This keeps secrets out of the app but would prevent
   the requested local key generation and import workflow. It could be added later.

The protocol library does not depend on HTTP, SQLite, the TUI, or the vault.
The relay does not import client code. The commands and TUI share services inside
`murm-cli`, without duplicating rules or adding another crate.

## Responsibilities and interfaces

| Plan | Responsibility | Provides |
| --- | --- | --- |
| [01](plans/01-protocol.md) | Events, JSON/JCS, signatures, HTTP types, filters, and revisions. | Types and deterministic validation. |
| [02](plans/02-relay.md) | Server-side SQLite, API, policies, cursors, and replication. | An HTTP relay for `publications-test-v1`. |
| [03](plans/03-client.md) | Local SQLite, profiles, the vault, drafts, networking, and commands. | `ClientService`, also used by the TUI. |
| [04](plans/04-tui.md) | Navigation, forms, and terminal tasks. | An interface over `ClientService`, with no SQL or cryptography in the views. |

The library exposes `EventId`, `SignerKey`, `Event`, `UnsignedEvent`,
`VerifiedEvent`, `ParseLimits`, `ProtocolError`, `PublicationKey`,
`PublicationMeta`, `EventQuery`, `SyncRequest`, `QueryResponse`,
`SyncResponse`, `RelayInfo`, `StoreReceipt`, `Problem`, and `ProblemCode`.
Construction of `VerifiedEvent` is controlled by verification. Cryptographic
verification does not validate editorial content.

Main contracts:

- `parse_and_verify(bytes: &[u8], limits: ParseLimits) -> Result<VerifiedEvent, ProtocolError>`.
- `sign_event(draft: &UnsignedEvent, seed: &[u8; 32]) -> Result<VerifiedEvent, ProtocolError>`.
- `validate_publication(event: &VerifiedEvent) -> Result<PublicationMeta, ProtocolError>`.
- `compare_revisions(a: &PublicationMeta, b: &PublicationMeta) -> Result<Ordering, ProtocolError>`
  rejects comparisons between different publications.
- `matches_filter(event: &VerifiedEvent, filter: &Filter) -> bool` runs after request validation.

Plan 03 defines the client types. Views do not receive seeds, passwords, master
keys, or a SQLite connection.

## Profiles and keys

Each local profile has an internal ID, a display name, a unique public key, a
mode, associated relays, and preferences. The public key is the actual identity;
the name can change without changing the identity. Signing and read-only mode
are local states, not permissions enforced by the relay.

Workflows:

- Create: generate a 32-byte Ed25519 seed with the system RNG and derive the public key.
- Import a private key: accept a seed of exactly 64 hex characters and derive
  the public key. If the user supplies a public key too, require an exact match.
- Import a public key: validate a 64-character hex public key and create a
  read-only profile without a private key.
- Reimport the same public key: do not duplicate the identity. A matching private
  key can upgrade the existing profile to signing while preserving relays and drafts.
- Rename, select, lock, unlock, or remove a local profile. Local removal never
  publishes a withdrawal or deletes content from relays.

Do not implicitly accept expanded 64-byte seeds, Nostr formats, recovery
phrases, or hardware signers. Report a format error instead of guessing.

## Local vault

Initialize a vault before creating the first signing profile:

- Generate a random 32-byte master key and encrypt it with a password-derived key.
- Use Argon2id v19: 65536 KiB of memory, 3 iterations, parallelism 1, 32-byte output,
  and a random 16-byte salt. Validate parameters before running the KDF. The v1
  reader accepts only this parameter set, so the file cannot prescribe arbitrary costs.
- Use XChaCha20-Poly1305 with a new random 24-byte nonce for each encryption.
  Never reuse a nonce with the same key.
- The wrapped master key authenticates the vault version and parameters.
  The encrypted seed authenticates the version, profile ID, and public key as associated data.
- Store only ciphertext, salt, nonce, and parameters in SQLite. Seeds and passwords
  must not appear in SQL, WAL, logs, errors, arguments, or environment variables.
- Require a password of at least 12 Unicode characters at creation. Do not normalize it.
  Unlocking accepts the existing password without applying new policies.
- The vault starts locked. Locking clears secrets from memory using zeroizing types.
  This does not guarantee erasure of swap, dumps, or existing copies.
- Changing the password rewraps the master key atomically. A wrong password or
  corruption does not overwrite the vault. There is no server recovery.
- Initialization, unlocking, and password changes are explicit vault operations.
  `create_profile` with a private key requires an already unlocked vault;
  it does not silently create one. The TUI and CLI guide first-time initialization.
- Each vault operation has a service-controlled token and generation. Locking
  or canceling invalidates the attempt under the same lock that installs the
  master key. The KDF runs outside that lock. Check the generation and token
  again before installing the result; a late response cannot unlock the vault.
  If initialization committed before cancellation, an empty locked vault may
  remain, but never an unlocked key or an overwritten file.
- Unlocking in memory does not encrypt drafts or public events. The SQLite file
  as a whole is not encrypted.

The password protects the local application; it is not a protocol login.
Read-only profiles do not need the vault to be initialized or unlocked.
Automatic backup and a secret export format are outside this stage.

## Persistence and concurrency

Use two independent databases, never the same file:

- Relay: events, monotonic admission sequence, preferred revisions, propagation
  queues, remembered IDs, sync state, and cursor secrets.
- Client: profiles, encrypted vault data, relays, drafts, verified events,
  preferences, cursors, and per-destination outbox state.

Use the platform data directory and accept `--data-dir` and `--db` for tests.
Use versioned migrations, `foreign_keys=ON`, WAL, and a five-second `busy_timeout`.
Each process has a connection managed by a dedicated worker. Async calls send work
to the worker; keep SQL and heavy KDF work out of the TUI loop. Neither program
opens the other's database. A migration error does not erase the database.

Use an exclusive client instance lock so two TUIs cannot unlock or edit the same
vault at once. Independent instances use separate directories. Short read-only
inspection connections are optional.

## Client and TUI

Running `murm-cli` without a subcommand opens the TUI in an interactive terminal.
With noninteractive input, it prints help without waiting indefinitely for a password.
Subcommands support the same workflows for scripts and tests.

The TUI includes profiles, profile creation and import, vault unlocking, relays,
articles, revision reading and history, drafts, and the outbox. Ratatui and asynchronous
Crossterm input provide an adaptive navigation sidebar, content area, and footer.
The UI shows the active profile, vault state, ongoing tasks, and errors without
interrupting rendering.

Publishing requires an unlocked signing profile. Read-only profiles can query,
sync, and read cached content offline. Switching profiles does not change the
identity associated with an existing task or outbox item: each carries its own
`ProfileId` and generation identifier. Stale results cannot replace the new
profile's view state.

Articles are Pandoc Markdown source. The reader shows scrollable text; it does
not promise full citation rendering. Press `e` to edit in the TUI's plain
source editor, powered by ratatui-textarea. The editor opens on Body; `Tab`
switches between Title and Body. `Ctrl+S` saves locally, `Ctrl+Z` undoes,
`Ctrl+Y` redoes, `Shift` plus arrows selects text, and `Ctrl+A` selects all
in the focused field. `Esc` goes back and `Ctrl+C` quits;
both require confirmation if there are unsaved changes. `p` confirms publishing
a saved draft. The optional command-line editor takes an executable and argument
list without using a shell, and preserves the saved draft on failure.

## Relay and client data flow

Enforce limits before processing untrusted data. The relay and client recalculate
IDs and verify signatures; unknown values are preserved. Receiving public
publication metadata does not certify the article's sources.

The relay acknowledges only after committing the event, admission, and revision
state. Replication is persistent and best-effort. The client reuses the signed
event on retry. Success means storage at the named destination. Outbox retries are
user-triggered and subject to backoff.

Sync commits events, preferred revisions, and the cursor in one transaction.
An invalid batch does not advance the cursor under MIP-03. Content expiry does
not erase remembered withdrawals or the verified preferred revision.

The relay's `import_sync_batch` is distinct from `admit`: it validates the
entire batch first, then admits events, revision heads, propagation jobs, and the
peer cursor in one transaction. Internal admission does not commit on its own.
An invalid last event or a commit failure cannot leave an advanced cursor or
partial batch entries. The client applies the same rule to its cache, heads, and cursor.

## First release limits

The release covers kind 1 publications, queries, sync, the current view, and
propagation among manually configured peers. It does not implement DMs or Double
Ratchet, automatic signed discovery, relay lookup, standardized backup, or device
delegation, nor does it promise strong blocking resistance. Local profiles do not
introduce a new event kind.

Acceptance tests use two relays and two client directories, covering crash and
restart, duplicates, concurrent edits, withdrawal, and expiry. Sharing a crate
does not show independent interoperability; also check the known MIPs
cryptographic example.

## Implementation order and review criteria

1. Shared protocol library.
2. Relay and client persistence and profiles after the interfaces in Plan 01.
3. Client networking and commands after the relay API.
4. TUI on top of tested services.
5. End-to-end failure scenarios and bottleneck measurements.

The component plans record the approved implementation sequence. Prototype
measurements and the final review are still pending.

## Technical references

- [Ratatui](https://ratatui.rs/): widgets and terminal rendering.
- [rusqlite](https://docs.rs/rusqlite/latest/rusqlite/): in-process SQLite.
- [Axum](https://docs.rs/axum/latest/axum/): HTTP layer.
- [serde_jcs](https://docs.rs/serde_jcs/latest/serde_jcs/): canonicalization.
- [Ed25519 dalek](https://docs.rs/ed25519-dalek/latest/ed25519_dalek/struct.VerifyingKey.html): verification behavior.
- [Argon2](https://docs.rs/argon2/latest/argon2/): key derivation.
- [ChaCha20Poly1305](https://docs.rs/chacha20poly1305/latest/chacha20poly1305/): AEAD and extended nonces.

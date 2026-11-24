# Murm

An experimental Rust implementation of the `publications-test-v1` profile from the [MIPs](https://github.com/mi66mc/mips).
It includes a shared protocol library, a persistent relay, and a client with command-line commands and a terminal UI (TUI).

An Ed25519 public key is the identity. The relay has no accounts, logins, or passwords.
The vault password protects the private key in the local app only.

## Components

- `murm-protocol`: strict JSON, JCS/SHA-256, Ed25519, publications and revisions, and HTTP contracts.
- `murm-relay`: SQLite, admission-based retention, queries, sync, and bounded propagation between configured peers.
- `murm-cli`: profiles, an encrypted vault, a separate SQLite database, drafts, an outbox, and a Ratatui TUI.

The client does not depend on the relay. The protocol library does not depend on SQLite, HTTP, or a terminal.
The normative reference is [MIPs commit c3d6018](https://github.com/mi66mc/mips/tree/c3d601848cd05c41b3726a9dc2631239656da8e7).

## Build or install just one program

The prototype was tested locally with Rust 1.97.0. The minimum supported version has not been verified separately.

```sh
cargo build --workspace --locked

cargo build -p murm-cli --locked
cargo install --path crates/murm-cli --locked

cargo build -p murm-relay --locked
cargo install --path crates/murm-relay --locked
```

Installing the CLI does not install the relay.

## Run two local relays

In two terminals, from the workspace root:

```sh
cargo run -p murm-relay -- --config examples/relay-a.toml
cargo run -p murm-relay -- --config examples/relay-b.toml
```

Relay A uses port 8080 and relay B uses 8081. They use separate databases and explicitly
configured peers. New events propagate automatically within the configured budget; there is no forwarding flag.

The HTTP server listens on loopback only. For remote access, put an HTTPS reverse proxy
in front of it. The server does not trust client-supplied `X-Forwarded-For` headers.
Remote peer and client addresses require HTTPS; plain HTTP is allowed only on loopback.
There is no automatic discovery or recursive lookup.

The configuration file can be TOML or JSON. Use JSON to specify `null` in retention policies.
`/info` advertises the effective configuration, including policies by kind, retention,
filters, limits, rate limits, and propagation.

## TUI

```sh
cargo run -p murm-cli -- --data-dir .murm-data/author
```

With no subcommand, the CLI opens the TUI in an interactive terminal. Otherwise, it prints help
without waiting for a password. The command line and TUI use the same database and services.
The TUI uses Ratatui and asynchronous Crossterm input, with navigation, a content area, and a
status and shortcut footer that adapt to the terminal size.

| Key | Action |
| --- | --- |
| `1`–`5` / `Tab` | Switch between Profiles, Articles, Drafts, Relays, and Outbox. |
| Arrows / `Enter` / `Esc` | Navigate, open or confirm, and go back or cancel. |
| Profiles: `n` / `i` / `o` | Generate a key, import a private key, or import a public key only. |
| `u` / `l` | Unlock or lock the vault. |
| Profiles: `r` / `d` / `P` | Rename, confirm local removal, or change the password. |
| Drafts: `n` / `e` / `p` | Create a draft, edit it in the TUI, or confirm publishing a saved draft. |
| Articles: `h` / `e` / `w` | Open history, create a revision or restoration, or confirm withdrawal. |
| Relays: `n` / `Enter` / `s` / `d` | Add, view `/info`, sync, or remove. |
| Outbox: `Enter` / `r` | Inspect destination outcomes, or retry pending destinations whose backoff has elapsed. |
| `F5` / `?` / `Ctrl+C` | Refresh, open help, or quit. |

In the article list, `Enter` reads cached source, even offline. History is separate from
the current view. A known withdrawal does not cause an older revision to reappear as current.
To restore an article, create a revision with `e`, write its full contents, save it locally,
then publish the saved draft.

The built-in editor opens on the Body field and edits plain Pandoc Markdown without
leaving the TUI. `Tab` switches between Title and Body. `Ctrl+S` saves locally,
`Ctrl+Z` undoes, and `Ctrl+Y` redoes. `Shift` plus arrow keys selects text;
`Ctrl+A` selects all text in the focused field. `Esc` returns to the previous view
and `Ctrl+C` exits; both ask for confirmation if there are unsaved changes.
Saving does not sign or publish anything. The reader shows source without running HTML,
filters, scripts, or external bibliographies.

On the Relays screen, `s` uses the selected relay. Elsewhere, it uses the profile's
first saved relay. An empty result does not trigger an automatic query to a different server.

The command line still supports an optional external editor, independent of the TUI.
Its default is Notepad on Windows and `vi` elsewhere. Configure an executable and arguments
to run without a shell:

```sh
murm-cli --data-dir .murm-data/author editor EDITOR_PATH --arg ARGUMENT
```

An external editor failure preserves the saved draft.

## Profiles and commands

Generate a test identity and add a relay:

```sh
murm-cli --data-dir .murm-data/author profile create Author
murm-cli --data-dir .murm-data/author relay add http://127.0.0.1:8080
murm-cli --data-dir .murm-data/author profile list
```

The command prompts for the local password without echoing it. The first profile is selected
automatically. Choose another with `profile use ID` or `--profile ID`.

Import a public key to create a read-only profile:

```sh
murm-cli --data-dir .murm-data/reader profile import Reader --public-key PUBLIC_KEY_HEX
```

Import a private key, optionally checking it against an existing public key:

```sh
murm-cli --data-dir .murm-data/author profile import Author --secret-file SECURE_SEED_PATH --public-key PUBLIC_KEY_HEX
```

The private key is a 32-byte Ed25519 seed written as 64 hex characters. The public key uses 64 lowercase
hex characters. A mismatched public key causes the import to fail. Importing the matching private key
upgrades an existing read-only profile while preserving its ID and data.

The client does not implicitly import expanded keys, recovery phrases, or Nostr/secp256k1 keys.
Supply a private key only by file, `--secret-stdin`, or a masked form; never pass the secret
as an argument. For automation, use `--password-file PATH`. Never put the password itself
in an argument or environment variable. Keep secret files out of the repository.

Other available commands:

```sh
murm-cli profile rename ID NEW_NAME
murm-cli profile remove ID --yes
murm-cli vault init
murm-cli vault passwd --new-password-file PATH

murm-cli article draft --title TITLE --source-file article.md
murm-cli article edit --draft ID --source-file article.md
murm-cli article edit --publication IDENTIFIER
murm-cli article publish DRAFT_ID
murm-cli article history IDENTIFIER
murm-cli article withdraw IDENTIFIER
murm-cli article restore IDENTIFIER --source-file article.md

murm-cli fetch EVENT_ID --relay CONFIGURED_URL
murm-cli query --current --relay CONFIGURED_URL
murm-cli query --request-file query.json --relay CONFIGURED_URL
murm-cli sync --relay CONFIGURED_URL
murm-cli outbox list
murm-cli outbox retry
```

Add `--data-dir` or `--db` to choose the database and `--profile` as needed.
A query request file contains an `EventQuery` from MIP-03, for example:

```json
{"filters":[{"kinds":[1],"header":[{"path":"/status","op":"eq","value":"published"}]}],"limit":20,"view":"current"}
```

The client saves the outbox before contacting the network. Publishing the same already queued draft
reuses its event. Retries do not change the ID, signature, or timestamp. `stored` means the
event was stored at that relay; it does not mean it was replicated, delivered, or read.
Removing a relay cancels its remaining pending destinations. Removing a profile deletes only its
local data, after confirmation, without withdrawing remote posts.

## Data, security, and limits

By default, the app uses the platform data directory for `org/Murm/murm`. The exact path
depends on the system; `--data-dir` and `--db` let you override it. The client and
relay use separate files. A lock prevents a second client from writing to the same database.

The vault encrypts the private key with XChaCha20-Poly1305 under a random master key. The local
password protects the master key using Argon2id v19 with 64 MiB, 3 iterations, and parallelism 1.
The app starts locked and uses zeroizing buffers for secret material. This does not guarantee
erasure of swap, dumps, backups, or earlier copies.

The SQLite database itself is not encrypted: articles, drafts, names, public keys, and configuration
are visible in the file. This release has no password recovery, export for generated keys, or
automatic backup. Use test identities.

Local prototype limits:

- Relay: SQLite defaults to 512 MiB, configurable with `max_storage_bytes`.
  A write failure does not produce a storage acknowledgement.
- Client: SQLite defaults to a 2 GiB limit, configurable with `--max-db-bytes`.
  The WAL and auxiliary files incur separate overhead.
- Public cache: up to 100,000 events or 512 MiB, and 100,000 publication states.
  There is no automatic compaction; a capacity refusal does not advance the cursor.
- Local lists display up to 1,000 records. Sync processes up to 16 pages per call and reports
  `has_more`. Retrying the outbox processes up to 100 entries per call.
- Each destination has an attempt limit of 10. Retries are user-triggered and subject to backoff;
  there is no automatic background retry. Permanent refusals are not retried without a change.
- Client networking allows up to 5 s to connect, 20 s per operation, and a 4 MiB response,
  with no automatic redirects. These are local limits, not universal protocol limits.

Kind 1 is the only kind the relay accepts under this profile. DMs/Double Ratchet, signed discovery,
lookup, backup/export, full Pandoc rendering, and strong blocking resistance are not implemented.
The protocol remains extensible through the MIPs.

A valid signature does not establish editorial truth, completeness of a response, permanence,
or that a version is the newest globally. The prototype has not undergone a security audit
and does not demonstrate interoperability with an independent implementation.

## Verification

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

End-to-end tests use isolated processes, local ports, and temporary directories.
They find the relay next to the client binary or through `MURM_RELAY_BIN`.
Tests do not run Cargo themselves. TUI tests cover both terminal-free rendering and a real pseudoterminal.

The [design](docs/design.md) and the [component plans](docs/plans/README.md) record the approved scope.

## License

GNU Affero General Public License version 3 only (AGPL-3.0-only).
See [LICENSE](LICENSE).

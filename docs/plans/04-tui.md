# Plan 04 — TUI, End-to-End Workflows, and Failure Tests

> Historical implementation plan. The original execution guidance was to use
> superpowers:executing-plans or superpowers:subagent-driven-development, one task at a time.
> The [TUI refresh](05-tui-refresh.md) supersedes the original external-editor design for the TUI.

**Goal:** Provide a terminal interface for profiles, articles, relays, and sync.
**Architecture:** Keyboard events and task results update AppState. Rendering
only reads state; actions use ClientService.
**Stack:** Ratatui/Crossterm, Tokio, ratatui-textarea for source editing, and TestBackend for tests.
**Spec:** [Design](../design.md), [client](03-client.md), and [relay](02-relay.md).

## Global constraints

Use the existing murm-cli crate. The TUI and commands share data and services.
Do not run SQL, KDFs, or signing in the draw loop. Do not write logs to the active
terminal. Widgets must not expose passwords or seeds, and snapshots must not
contain real secrets. Installing only the CLI does not build or install the relay.

## Review focus

1. A crash or cancellation leaves the terminal in raw mode or hides the cursor.
2. A profile switch applies a network response to the wrong profile.
3. The UI freezes during KDF work, SQLite operations, or slow networking.
4. A read-only profile offers publishing or signing actions.
5. Key repeats or timeouts duplicate a revision or publication.

## Files and interfaces

Create src/tui/mod.rs, state.rs, actions.rs, render.rs, terminal.rs, tasks.rs,
views/profiles.rs, views/unlock.rs, views/relays.rs, views/articles.rs,
views/drafts.rs, and views/outbox.rs. These modules use Plan 03's ClientService.

AppState holds Screen, the active ProfileSummary, public vault state,
UiGeneration, items, selection and scroll state, and tasks. It does not hold
persistent secrets. Sensitive form fields use zeroizing buffers discarded on
confirmation or cancellation. Action expresses intent. Tasks carry TaskId,
ProfileId, and generation.

Screen includes Profiles, ProfileForm, Unlock, Relays, Articles, Article,
Drafts, Outbox, and a removal confirmation. Enter confirms, Esc goes back or
cancels, arrows and PageUp/PageDown navigate, Tab changes focus, ? shows help,
and Ctrl+C quits. Use asynchronous Crossterm input and adapt the Ratatui
sidebar, content area, and footer to the terminal size. All UI copy is in English.
Ratatui is an interface library, not a Murm content format.

### Task 1: State, navigation, and terminal recovery

**Files:** tui/state.rs, actions.rs, terminal.rs, tests/tui_navigation.rs.

- [ ] Test starts_at_profiles_when_empty, escape_restores_previous_screen,
  noninteractive_prints_help, ctrl_c_exits_cleanly, and terminal_restored_after_error.
- [ ] Run cargo test -p murm-cli --test tui_navigation; expect failures.
- [ ] Implement AppState and a reducer. A terminal guard restores raw mode,
  the alternate screen, and the cursor after normal exit, errors, and
  recoverable panics. Do not promise restoration after a forced process kill.
- [ ] Do not run network, database, or KDF work while drawing. Use a bounded
  channel for task events, cancellation, and orderly worker shutdown.
- [ ] Run the tests and commit: feat(tui): add terminal lifecycle and navigation.

### Task 2: Profiles, import, and unlocking

**Files:** views/profiles.rs, views/unlock.rs, tasks.rs, tests/tui_profiles.rs.

- [ ] Test generate_profile_flow, import_pair_rejects_mismatch,
  import_public_is_read_only, duplicate_public_promotes_existing_profile,
  secrets_masked_and_cleared, unlock_wrong_password_keeps_profiles,
  changing_profile_discards_stale_ui_results, and lock_removes_signing_actions.
- [ ] Run cargo test -p murm-cli --test tui_profiles; expect failures.
- [ ] Show profiles with their name, abbreviated public key, mode, and locked
  state. Offer separate workflows for identity generation, private-key import
  with an optional public key, and read-only public-key import. Make the full
  public key available for verification.
- [ ] Mask password and private-key forms. Do not implicitly copy or display
  a private key. Limit input size before decoding, and do not echo secrets in
  errors. Confirm local removal and explain that nothing was removed from relays.
- [ ] Run the KDF on a worker without blocking the interface. On cancellation,
  discard the result and secrets; do not let a late response unlock the vault.
  Use the service's VaultActionId, not just UiGeneration. Esc calls
  cancel_vault_action. Before the first Signing profile, show password creation
  and confirmation, and call initialize_vault. If cancellation follows the
  commit of an empty vault, keep it locked and reflect VaultStatus on screen.
- [ ] Run the tests and commit: feat(tui): add local profile and vault flows.

### Task 3: Relays, articles, drafts, and outbox

**Files:** views/relays.rs, articles.rs, drafts.rs, outbox.rs, tests/tui_articles.rs.

- [ ] Test offline_cache_is_readable, relay_settings_are_profile_scoped,
  history_is_distinct_from_current, withdraw_requires_owner,
  read_only_can_sync_but_not_publish, repeated_confirm_queues_once, and
  status_shows_local_storage_not_delivery. Include
  terminal_escape_sequences_render_as_text and unicode_navigation_does_not_panic.
- [ ] Run cargo test -p murm-cli --test tui_articles; expect failures.
- [ ] Show configured relays, capabilities and policies from /info, ongoing
  sync, and the last error. An empty result must not start a query to another
  relay without explicit user action or configuration.
- [ ] List articles in the current view, with separate history, editing,
  withdrawal and restoration, drafts, and the outbox. Render Pandoc source
  as scrollable text without running HTML or filters. Escape control characters
  from articles, metadata, and responses; remote text must not emit ANSI or OSC
  sequences into the terminal. Draw only the visible region and handle Unicode
  positions without slicing through a character.
- [ ] The original design called for a configurable executable and argument list
  with no shell, passing a temporary file as a separate argument. It released
  the terminal before launching the editor and restored it afterward; failure
  or cancellation did not sign anything or discard the saved draft. That
  external editor is now optional for command-line use only.
- [ ] In the TUI, `e` opens the integrated editor for plain Pandoc Markdown
  source using ratatui-textarea. The editor opens on Body; `Tab` switches between
  Title and Body and `Ctrl+A` selects all in the focused field.
  `Ctrl+S` saves locally, `Ctrl+Z` undoes, `Ctrl+Y` redoes, and
  `Shift` plus arrows selects text. `Esc` returns and `Ctrl+C` quits;
  both ask for confirmation if there are unsaved changes. Saving does not sign
  or publish; `p` publishes only a saved draft after confirmation.
- [ ] Show the profile and public key, title and revision, and destinations
  in the send confirmation. The task uses the captured ProfileId rather than
  reading the active profile after waiting for the network.
- [ ] Run the tests and commit: feat(tui): add relay and publication screens.

### Task 4: Binary integration and end-to-end tests

**Files:** main.rs, tests/end_to_end.rs, and the project README.md.
**Uses:** The complete test relay and the preceding services and TUI.

- [ ] Write two_relays_two_clients, create_profile_publish_read,
  stop_origin_read_replica, restart_retains_profiles_and_events,
  offline_edit_then_same_event_retry, withdrawal_and_body_expiry,
  stale_cursor_recovers, and storage_failure_does_not_ack.
- [ ] Run cargo build --workspace before cargo test --workspace --test end_to_end;
  expect the tests to fail initially. The test lives in murm-cli. Find the relay
  next to CARGO_BIN_EXE_murm-cli with the platform suffix, or accept an explicit
  MURM_RELAY_BIN. Do not run Cargo from within the test. The CLI executable does
  not depend on the murm-relay crate.
- [ ] Open the TUI when murm-cli is run without arguments in an interactive
  terminal. Subcommands still work without drawing the TUI. Handle startup
  errors before entering raw mode.
- [ ] Change only the integration needed. Use ephemeral loopback ports,
  separate temporary directories, and an injected clock. Do not use real keys
  or launch persistent instances on the computer during tests.
- [ ] Run cargo test --workspace, cargo fmt --all -- --check, and
  cargo clippy --workspace --all-targets -- -D warnings. Build with
  cargo build -p murm-cli separately and confirm that the relay is not installed.
- [ ] Document how to run two relays and a client, data paths, profile generation
  and import, the vault, read-only mode, and prototype limits.
- [ ] Commit: test: validate the relay and TUI publication workflow.

### Task 5: Measure bottlenecks after validating correctness

**Files:** docs/prototype-results.md (reserved for the pending measurements report).

- [ ] Measure batches of 100, 1000, and 10000 events with 1 KiB and 100 KiB
  sources. Respect /info and rate limits and record the policy used.
- [ ] Measure p50 and p95 for publishing, queries, and sync, along with
  verification cost, replication queue, memory use, and database growth.
- [ ] Repeat with an offline peer and short retention. Check that budgets
  remain finite and the TUI stays responsive.
- [ ] Record results and limits. Distinguish implementation bottlenecks from
  gaps in the MIPs, and make any needed MIP corrections through reviewed changes.
- [ ] Commit: docs: record prototype measurements.

## Exit criteria

Users can create, import, and select profiles; unlock signing; edit, publish,
and withdraw articles; and read or sync from another relay through the TUI.
The client remains separately installable. Do not claim support for DMs, full
Pandoc rendering, or independent interoperability.

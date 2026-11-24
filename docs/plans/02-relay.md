# Plan 02 — Persistent HTTP Relay

> Historical implementation plan. The original execution guidance was to use
> superpowers:executing-plans or superpowers:subagent-driven-development, one task at a time.

**Goal:** Serve publications-test-v1 with SQLite and bounded propagation.
**Architecture:** HTTP → shared validation → service → SQLite worker.
Retention, cursors, and queues are relay state, outside the signed event.
**Proposed stack:** Tokio, Axum, Rusqlite with bundled SQLite, Reqwest,
HMAC/SHA-256, base64url, TOML configuration, and structured logs without secrets.
**Spec:** [Design](../design.md), [Plan 01](01-protocol.md), and MIPs 03–06 at the
[reference commit](https://github.com/mi66mc/mips/tree/c3d601848cd05c41b3726a9dc2631239656da8e7).

## Global constraints

AGPL-3.0-only; events verified by murm-protocol; kind 1 only.
No login. /info advertises effective configuration; lookup and announcements are disabled.
Plain HTTP is for loopback testing only. For remote access, use TLS directly
or an HTTPS reverse proxy with a local connection. Do not trust arbitrary
X-Forwarded-For headers.

## Review focus

1. Acknowledgement before commit, or an event with no admission record.
2. A restart that loses cursors, queues, or remembered withdrawals.
3. A current query that selects an old version after filtering metadata.
4. Byte-limited pagination that skips an event or repeats an empty page.
5. Queues or retries that amplify copies between two relays indefinitely.

## Files and interfaces

Create src/lib.rs, config.rs, service.rs, http.rs, cursor.rs, replication.rs,
storage/mod.rs, storage/migrations.rs, storage/events.rs, and storage/query.rs.
Modify main.rs and Cargo.toml.

RelayConfig contains the complete /info shape and local fields: database_path,
listen, peers, trusted_proxy (disabled by default), backoff and timeout, and
aggregate limits for snapshots and queues. Invalid configuration prevents startup;
the relay must not advertise false limits.

`Clock::now_unix() -> u64` is injectable in services and tests; no sleeps are
needed to test expiry. RelayService takes commands from HTTP and coordinates
validation. RelayStore, on the worker, offers synchronous operations:

- `admit(&VerifiedEvent, &AdmissionPolicy, now: u64) -> Result<StoreReceipt, StoreError>`.
- `fetch(EventId, now: u64) -> Result<Option<VerifiedEvent>, StoreError>`.
- `query(&EventQuery, now: u64) -> Result<QueryResponse, StoreError>`.
- `sync(&SyncRequest, now: u64) -> Result<SyncResponse, StoreError>`.
- `import_sync_batch(peer: &PeerId, filters: &FilterSet, batch: ValidatedSyncBatch, now: u64) -> Result<SyncImportReport, StoreError>`.
  ValidatedSyncBatch is internal to the relay. It contains verified events,
  recordable semantic and policy decisions, and the source cursor. It is produced
  only after the batch is validated.

These names are the plan's contract. Use the library types rather than recreating
Event, filters, or error codes in the server.

### Task 1: Configuration, migrations, and worker

**Files:** config.rs, storage/mod.rs, storage/migrations.rs, tests/storage.rs, main.rs.
**Provides:** RelayConfig::load(path), RelayStore::open(path), and the database worker.

- [ ] Test rejects_inconsistent_info_policy, migrations_are_repeatable,
  corrupt_database_not_recreated, and sqlite_worker_does_not_block_runtime.
- [ ] Run cargo test -p murm-relay --test storage; expect failures.
- [ ] Create tables: relay_meta (epoch and HMAC secret), events (unique ID,
  complete verified JSON, signer/kind/time, admission_seq, admitted_at, deadlines),
  publication_heads (logical key, highest revision/ID/status),
  propagation_jobs (event_id, peer, attempts, next_try, deadline),
  seen_ids (id, expires_at), peer_sync (peer, filter_hash, cursor),
  and query_snapshots (id, query_hash, ordered IDs, position, expiry).
  admission_seq is monotonic and never reuses numbers after deletion.
- [ ] Apply foreign keys, WAL, and the design's timeout. Preserve the HMAC
  and epoch state. Migrations use user_version; configuration rejects impossible
  byte limits or deadlines.
- [ ] Run the tests and commit: feat(relay): add persistent relay storage.

### Task 2: Admission, retention, and current state

**Files:** storage/events.rs, service.rs, tests/admission.rs.
**Uses:** Validated protocol data, the effective policy per kind, and Clock.

- [ ] Test commit_before_receipt, duplicate_does_not_renew,
  duplicate_has_no_new_sequence, reaccept_evicted_gets_new_sequence,
  future_timestamp_policy, minimum_and_maximum_retention,
  removal_exception_is_explicit, head_survives_body_expiry,
  higher_revision_restores, and full_head_budget_refuses_new_work.
- [ ] Run cargo test -p murm-relay --test admission; expect failures.
- [ ] Implement a transaction that stores the event, admission, and preferred
  revision and prepares propagation work if capacity permits. Only then return
  stored. A full queue does not roll back storage; it only prevents new jobs.
  Admission also validates kind 1 metadata before updating heads, including when
  called by the sync importer. Duplicates are validated but keep their contents,
  deadline, and sequence.
- [ ] Expire content at its admission deadline while preserving publication_heads.
  If the winning revision's body is missing, current omits the publication rather
  than promoting an old revision. Preference does not use created_at.
  A write failure does not return 201.
- [ ] Run the tests, reopen the connection in the test, and commit:
  feat(relay): enforce durable admission and retention.

### Task 3: Queries, snapshots, and synchronization

**Files:** storage/query.rs, cursor.rs, tests/query_sync.rs.
**Provides:** Query and sync, plus authenticated opaque tokens.

- [ ] Test filters_match_reference_implementation, raw_keyset_ties,
  current_selected_before_filters, current_snapshot_under_updates,
  late_old_event_appears_in_sync, wrong_filter_cursor_rejected,
  cursor_expires_explicitly, sync_survives_restart,
  response_byte_limit_does_not_skip, and expired_rows_do_not_loop.
- [ ] Run cargo test -p murm-relay --test query_sync; expect failures.
- [ ] Translate only validated filters to parameterized SQL; do not interpolate
  arbitrary paths. Evaluate structured equality using the library's JCS semantics.
  Raw view orders by descending created_at and ID with keyset pagination.
- [ ] Current view selects heads before filtering and pins the result IDs in a
  snapshot. Limit aggregate snapshot count and bytes in addition to the
  per-snapshot ceiling. Refuse work at capacity rather than grow memory unbounded.
- [ ] Tokens contain an internal version, type, epoch, request or filter hash,
  limit where applicable, position, and expiry. Authenticate them with HMAC.
  Query binds the limit and view; sync allows only the batch size to change.
  A forged token returns invalid_cursor; expiry or lost history returns
  cursor_expired. Do not expose SQL or secrets.
- [ ] Sync uses admission_seq, persists state, and never advances past an event
  that did not fit in the page. The next cursor may move past expired records.
- [ ] Run the tests and commit: feat(relay): add query and resumable synchronization.

### Task 4: API, errors, and limits

**Files:** http.rs, service.rs, tests/http_api.rs.
**Provides:** A server with all endpoints in the profile.

- [ ] Test single_event_post, local_get_404, query_and_post_equivalent,
  query_sync_and_post_equivalent, fixed_window_shared_between_aliases,
  invalid_event_problem_details, unsupported_feature_explicit,
  source_ip_not_untrusted_forwarded_header, and bounded_body_before_parse.
- [ ] Run cargo test -p murm-relay --test http_api; expect failures.
- [ ] Implement QUERY as a custom HTTP method accepted by the HTTP layer.
  Do not depend on the framework's MethodFilter recognizing the token.
  Expose /info, /events, /events/{id}, /events/query, and /events/sync.
  /relays and lookup=true return unsupported_feature.
- [ ] 201/200 and Location/retain_until follow MIP-03. Problem Details preserves
  the correct code and status. Malformed JSON, wrong IDs, and invalid signatures
  do not become policy_rejected. Enforce limits on decompressed input, depth,
  output, filters, snapshots, and admission before the corresponding work.
- [ ] Rate windows use UTC and share budgets across aliases.
  Use Retry-After for 429; there are no protocol credentials.
- [ ] Run the tests and commit: feat(relay): expose the publication test API.

### Task 5: Propagation and recovery

**Files:** replication.rs, tests/replication.rs.
**Uses:** Configured peers, the persistent queue, and the shared API.

- [ ] Test two_relays_converge_without_loop, retries_are_globally_bounded,
  duplicate_does_not_reopen_job, queue_full_keeps_receipt, restart_resumes_job,
  retry_after_respected, peer_sync_restores_outage,
  invalid_last_event_leaves_batch_uncommitted, and failed_batch_commit_keeps_cursor.
- [ ] Run cargo test -p murm-relay --test replication; expect failures.
- [ ] Send the same event to configured peers. Track per-event attempts across
  destinations, use timeouts within the deadline, and increase backoff.
  A duplicate or 201 acknowledges the destination. Other 4xx responses are
  not retried without a change, except 408 and 429. Retry 5xx and network
  failures within the budget.
- [ ] Recover by syncing with a stable kind 1 filter and a separate cursor per
  peer. Apply MIP-03 rules for invalid batches and policy. Commit events and
  cursor together; do not do recursive lookup or implicit discovery.
  Use import_sync_batch rather than multiple independently committing calls
  to admit. Share an admit_in_transaction helper between the two operations.
  Validate the whole batch before changing anything. An invalid event from a
  source that claims to validate it rejects the whole batch. A transaction
  failure rolls back that batch's events, heads, jobs, and cursor.
  Record local rejections allowed by MIP-03 without storing the event as valid.
- [ ] Run the tests and commit: feat(relay): add bounded propagation and recovery.

## Exit criteria

Run cargo test -p murm-relay and cargo fmt --all -- --check.
The process starts with murm-relay --config relay-a.toml. Instances use separate
databases and explicit peers. Test configuration shortens TTL without making
/info inaccurate. Do not put user seeds on the server or launch the TUI from it.

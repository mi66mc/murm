# Murm Implementation Plans

The prototype is implemented and in final validation. These plans preserve the
approved roadmap. Measurements and final review remain pending. Start with the
[shared design](../design.md) for the project decisions and implementation choices.

1. [Shared protocol](01-protocol.md): events, signatures, types, and rules.
2. [Relay](02-relay.md): SQLite, HTTP, policies, cursors, and replication.
3. [Client and profiles](03-client.md): local SQLite, key generation and import,
   the vault, drafts, transport, and commands.
4. [TUI and integration](04-tui.md): screens, asynchronous tasks, and end-to-end tests.
5. [Responsive TUI and editor](05-tui-refresh.md): immediate input, a readable layout, and integrated Markdown editing.

The implementation order was 01, then 02 and 03 as their interfaces became
available, and finally 04. Load testing follows behavior and failure validation.

The user approved implementation. Dependency versions are locked in
`Cargo.lock`. The [README](../../README.md) documents current commands,
limits, and features.

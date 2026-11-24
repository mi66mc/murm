# Responsive TUI and Markdown editing

Updated on 2026-11-23. This updates the local prototype interface; it is not a protocol security audit or completion of the separate load-testing plan.

## What changed

- Asynchronous input is independent of the 16 ms rendering budget. A burst no longer incurs an 80 ms delay per event.
- Base-screen navigation remains available while background tasks run. Late results cannot navigate over a newer user-selected screen.
- Wide terminals use a sidebar, compact lists, and details. Smaller terminals use compact navigation. The editor keeps a visible body at 40×12.
- Markdown drafts are edited inside the TUI. The optional external-editor CLI command remains available.
- Application copy and repository Markdown are English. Existing articles and local names are not translated.

## Editor

Open a draft with e. Tab changes title/body focus. Ctrl+S saves locally without signing or publishing. Shift+arrows selects text, Ctrl+A selects the focused field, and Ctrl+Z/Ctrl+Y undo/redo. Selection replacement uses the widget's separate delete/insert history steps.

Esc returns and Ctrl+C exits. Unsaved buffers require an explicit y to discard; n or Esc keeps editing. Failed or conflicting saves preserve the buffer. A save briefly freezes input until its version is confirmed.

Publishing remains a separate p action from the drafts screen and uses the saved draft. Outbox Enter opens scrollable relay outcomes, including refusals and retry information, at any terminal width.

## Validation

The final workspace run passed 196 tests with no failures. Formatting and Clippy with warnings denied also passed. This includes the real pseudoterminal navigation-burst test.

Regressions cover busy navigation bursts, key releases, late results, hidden Help actions, Unicode paste and selection, replacing a 3 MiB body, save conflicts, unsaved exit, compact layouts, and terminal-control escaping. The real pseudoterminal test sends 100 navigation keys before exiting with a two-second deadline.

    cargo build --workspace --locked
    cargo test --workspace --locked
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --locked -- -D warnings

A fresh read-only review found four significant issues: hidden actions behind Help, an invisible body at 40×12, inaccessible compact Outbox details, and replacement length accounting. Each has a regression that failed before correction. The long-selection case also exposed the widget's 16-bit select_all jump; the app now selects using document boundaries.

## Decisions and remaining polish

- Use ratatui-textarea rather than build another text engine. This adds a UI dependency but retains Ratatui 0.30 and the existing services.
- Relay/protocol production readiness and earlier unrelated prototype QA are outside this UI review. Existing experimental limitations still apply.
- Build/test evidence comes from the main session; the read-only reviewer did not rerun the suite independently.
- Deferred minor: Esc can retrace recent base-screen tabs; history is bounded to 16 entries. Direct screen shortcuts remain available.
- Deferred minor: some shortcut hints are uppercase while ordinary bindings are lowercase. Shift+P remains the distinct password-change binding.

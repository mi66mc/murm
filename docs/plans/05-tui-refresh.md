# Responsive TUI and integrated Markdown editor

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove queued screen navigation, improve readability, and edit articles without leaving the terminal.

**Architecture:** Keep Ratatui/Crossterm and the existing ClientService. An asynchronous terminal event stream wakes immediately; a short frame budget batches input before rendering. Local navigation stays available during background work. Task results may update data but may not navigate away from a newer user-selected screen. Use ratatui-textarea for Unicode-aware Markdown editing, selection, undo and wrapping; SQLite saves keep optimistic draft version checks.

**Spec:** The user approved the following design on 2026-11-22: independent input/rendering, background-safe navigation, a responsive sidebar/content/footer layout, integrated plain Markdown editing with Ctrl+S and unsaved-change protection, and English application/documentation copy.

## Approaches and constraints

- Selected: extend the current Ratatui application and use its maintained textarea widget. Preserve services, databases, identities, and protocol conventions.
- Rejected: retain the external editor as the primary workflow; it does not satisfy in-terminal editing. Keep the CLI external-editor command as an optional capability.
- Rejected: build a rich Pandoc renderer now; plain source editing is the approved scope.
- All application copy and repository Markdown must be English. User article content is never translated.
- Save is local only, never signs or publishes. A failed/conflicting save retains the editor buffer.
- Preserve the current feature branch and all existing changes and test data. No push, merge, or process termination without necessity/authority.

## Review focus

- Bursts of navigation and key release/repeat events must not produce an 80 ms per-key backlog.
- Late network results must not replace a newer screen or clear an unsaved editor.
- Unicode, multiline paste, undo/redo, and literal digits must work in the editor without triggering app shortcuts.
- Save failure, concurrent draft changes, and leaving/exiting with unsaved changes must not silently lose text.
- Narrow terminals and long remote labels must remain readable and must not emit terminal control sequences.

### Task 1: Responsive terminal input and safe navigation

**Files:** runtime.rs, state.rs, actions.rs, terminal input module, CLI Cargo.toml; tests/tui_navigation.rs and tests/end_to_end.rs.
**Interfaces:** Existing handle_key(AppState, KeyEvent), TaskContext, finish_task, ClientService stay public. Add terminal event batch dispatch and screen-generation-aware task results.

- [ ] Add and run failing tests for navigation while busy, burst routing, release filtering, and late results preserving the newer screen.
- [ ] Replace interval-gated input with EventStream, bounded input batching and render-on-change; ignore release events and avoid accumulating base-screen history.
- [ ] Keep background tasks bounded and mutations serialized; allow navigation/help while work runs. Preserve data refresh without unexpected navigation.
- [ ] Run cargo test -p murm-cli and commit the verified change.

### Task 2: Integrated Markdown editor

**Files:** tui/editor.rs, actions.rs, tasks.rs, runtime.rs, state.rs; tests/tui_editor.rs.
**Interfaces:** DraftEditor::new(Draft), source(), input(KeyEvent), paste(&str), draft_for_save(); UiCommand::OpenEditor / SaveDraft; UiMessage::Editor / DraftSaved.

- [ ] Add and run failing tests for editing, literal shortcuts, multiline Unicode paste, selection undo/redo, save persistence with no outbox change, stale-version failure retaining text, and discard confirmation.
- [ ] Add ratatui-textarea compatible with Ratatui 0.30. Use title/body focus, Ctrl+S save, Ctrl+Z undo, Ctrl+Y redo, Esc return and explicit unsaved discard/exit confirmation. Keep editing available without unlocking the signing vault.
- [ ] Save through ClientService::save_draft; protect concurrent edits during a save by disabling buffer mutation until completion. Saving does not publish.
- [ ] Run cargo test -p murm-cli and commit.

### Task 3: Layout, English copy, documentation, verification

**Files:** tui/render.rs, tui/views/*, remaining UI copy, README.md, docs/*.md, docs/plans/*.md, CLI UI tests.
**Interfaces:** Keep render(Frame, &AppState). Wide terminals use sidebar/list/details; narrow terminals use compact navigation/content. Editor takes the content area.

- [ ] Add and run failing layout tests at 120x36, 80x24 and 40x12, plus empty states, remote-control escaping and editor focus/cursor visibility.
- [ ] Implement consistent colors, compact metadata, contextual actions, clear loading/save/error indicators and readable selection/empty states.
- [ ] Translate repository Markdown and application copy to English; document the new shortcuts and workflow. Preserve historical decisions as historical.
- [ ] Run cargo build --workspace --locked, cargo test --workspace --locked, cargo fmt --all -- --check, cargo clippy --workspace --all-targets --locked -- -D warnings and git diff --check. Exercise the real terminal using its isolated pseudoterminal test and a navigation burst.
- [ ] Obtain one fresh read-only review of the implemented change, fix important findings with regression tests, and commit. Do not push or mark the separate prototype performance work complete.

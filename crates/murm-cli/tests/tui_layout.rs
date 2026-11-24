use murm_cli::{tui::*, *};
fn text(state: &AppState, width: u16, height: u16) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| render(f, state)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
#[test]
fn empty_states_remain_readable_at_common_sizes() {
    for (width, height) in [(120, 36), (80, 24), (40, 12)] {
        let mut state = AppState::default();
        for (screen, empty) in [
            (Screen::Profiles, "No profiles yet"),
            (Screen::Articles, "No articles yet"),
            (Screen::Drafts, "No drafts yet"),
            (Screen::Relays, "No relays yet"),
            (Screen::Outbox, "Nothing queued"),
        ] {
            state.screen = screen;
            let rendered = text(&state, width, height);
            assert!(rendered.contains(empty), "{width}x{height}: {rendered}");
            assert!(rendered.contains("Murm"));
        }
    }
}
#[tokio::test]
async fn editor_shows_source_focus_and_unsaved_warning() {
    let dir = tempfile::tempdir().unwrap();
    let app = ClientService::open(dir.path()).unwrap();
    let profile = app
        .create_profile(
            "Writer",
            ProfileInput::ImportPublic(murm_protocol::derive_public_key(&[1; 32])),
        )
        .await
        .unwrap();
    let draft = app
        .create_draft(profile.id, "Sample title", "# Source\n\nHello Markdown")
        .await
        .unwrap();
    let mut state = AppState::default();
    state.editor = Some(DraftEditor::new(draft).unwrap());
    state.screen = Screen::Editor;
    for (width, height) in [(120, 36), (80, 24), (40, 12)] {
        let rendered = text(&state, width, height);
        assert!(rendered.contains("Sample title"));
        assert!(
            rendered.contains("# Source"),
            "{width}x{height}: {rendered}"
        );
        assert!(rendered.contains("Ctrl+S"));
        assert!(rendered.contains("Markdown"));
    }
    state.editor.as_mut().unwrap().paste("change").unwrap();
    handle_key(
        &mut state,
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert!(text(&state, 80, 24).contains("Unsaved changes"));
}
#[test]
fn remote_labels_are_not_terminal_commands() {
    let mut state = AppState::default();
    state.profiles = vec![ProfileSummary {
        id: ProfileId::new_v4(),
        name: "Name\u{1b}]52;c;bad\u{07}".into(),
        public_key: murm_protocol::derive_public_key(&[1; 32]),
        mode: ProfileMode::ReadOnly,
        locked: false,
        relays: vec![],
    }];
    let rendered = text(&state, 120, 36);
    assert!(!rendered.contains('\u{1b}'));
    assert!(!rendered.contains('\u{07}'));
}

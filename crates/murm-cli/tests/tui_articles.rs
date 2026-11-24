mod common;
use common::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use murm_cli::tui::*;
use murm_cli::*;
use murm_protocol::*;
fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}
async fn writer() -> (tempfile::TempDir, ClientService, ProfileSummary) {
    let d = tempfile::tempdir().unwrap();
    let app = ClientService::open(d.path()).unwrap();
    app.initialize_vault(
        SecretString::new("uma senha longa de teste".into()),
        app.begin_vault_action(),
    )
    .await
    .unwrap();
    let p = app
        .create_profile("Ana", ProfileInput::Generate)
        .await
        .unwrap();
    app.add_relay(p.id, "http://127.0.0.1:9").await.unwrap();
    (d, app, p)
}
fn screen_text(state: &AppState) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| render(f, state)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
#[tokio::test]
async fn offline_cache_is_readable() {
    let (_d, app, p) = writer().await;
    let draft = app
        .create_draft(p.id, "Title", "Offline article source")
        .await
        .unwrap();
    let id = app.publish(p.id, draft.id).await.unwrap();
    let message = execute_command(&app, UiCommand::Read(id), Some(p.id))
        .await
        .unwrap();
    let mut state = AppState::default();
    state.switch_profile(p);
    let context = state.begin_task();
    assert!(state.apply_result(context, message));
    assert!(screen_text(&state).contains("Offline article source"));
}
#[tokio::test]
async fn relay_settings_are_profile_scoped() {
    let (_d, app, p) = writer().await;
    let other = app
        .create_profile(
            "Bruno",
            ProfileInput::ImportPublic(derive_public_key(&[9; 32])),
        )
        .await
        .unwrap();
    let mut form = Form::new(
        FormKind::AddRelay,
        app.vault_status().await.unwrap(),
        Some(other.clone()),
    );
    form.type_text("http://127.0.0.1:12345");
    execute_form(&app, form.take_request(), None).await.unwrap();
    assert_eq!(
        app.profile(p.id).await.unwrap().relays,
        vec!["http://127.0.0.1:9"]
    );
    assert_eq!(
        app.profile(other.id).await.unwrap().relays,
        vec!["http://127.0.0.1:12345"]
    );
}
#[tokio::test]
async fn history_is_distinct_from_current() {
    let (_d, app, p) = writer().await;
    let d = app.create_draft(p.id, "Title", "body").await.unwrap();
    app.publish(p.id, d.id).await.unwrap();
    app.withdraw(p.id, &d.publication).await.unwrap();
    let snapshot = load_snapshot(&app, Some(p.id)).await.unwrap();
    assert_eq!(snapshot.articles.len(), 1);
    assert_eq!(snapshot.articles[0].status, PublicationStatus::Withdrawn);
    let message = execute_command(
        &app,
        UiCommand::History(p.public_key, d.publication),
        Some(p.id),
    )
    .await
    .unwrap();
    if let UiMessage::History(events) = message {
        assert_eq!(events.len(), 2);
    } else {
        panic!("expected separate history");
    }
}
#[tokio::test]
async fn withdraw_requires_owner() {
    let (_d, app, p) = writer().await;
    let d = app.create_draft(p.id, "Title", "body").await.unwrap();
    app.publish(p.id, d.id).await.unwrap();
    let mut state = AppState::default();
    state.apply_snapshot(load_snapshot(&app, Some(p.id)).await.unwrap());
    state.screen = Screen::Articles;
    assert!(matches!(
        command_for_key(&state, key('w')),
        Some(UiCommand::Withdraw(_, _))
    ));
    state.active.as_mut().unwrap().public_key = derive_public_key(&[9; 32]);
    assert!(command_for_key(&state, key('w')).is_none());
}
#[tokio::test]
async fn read_only_can_sync_but_not_publish() {
    let (_d, app, p) = writer().await;
    let mut state = AppState::default();
    state.apply_snapshot(load_snapshot(&app, Some(p.id)).await.unwrap());
    state.active.as_mut().unwrap().mode = ProfileMode::ReadOnly;
    state.screen = Screen::Articles;
    assert!(matches!(
        command_for_key(&state, key('s')),
        Some(UiCommand::Sync(_, _))
    ));
    assert!(!state.can_sign());
    state.screen = Screen::Drafts;
    assert!(command_for_key(&state, key('p')).is_none());
}
#[tokio::test]
async fn repeated_confirm_queues_once() {
    let relay = server(vec![]).await;
    let (_d, app, p) = writer().await;
    app.remove_relay(p.id, "http://127.0.0.1:9").await.unwrap();
    app.add_relay(p.id, &relay.url).await.unwrap();
    let draft = app.create_draft(p.id, "Title", "body").await.unwrap();
    let id = app.publish(p.id, draft.id).await.unwrap();
    relay.script.lock().unwrap().responses.push_back((
        201,
        serde_json::to_string(&StoreReceipt {
            id,
            status: StoreStatus::Stored,
            retain_until: Some(2000000000),
        })
        .unwrap(),
    ));
    for _ in 0..2 {
        execute_command(&app, UiCommand::Publish(p.id, draft.id), Some(p.id))
            .await
            .unwrap();
    }
    assert_eq!(app.outbox(p.id).await.unwrap().len(), 1);
    assert_eq!(relay.script.lock().unwrap().requests.len(), 1);
}
#[test]
fn status_shows_local_storage_not_delivery() {
    let status = DestinationStatus {
        relay: "https://relay.example".into(),
        state: DestinationState::Stored,
        retain_until: Some(100),
        retry_at: None,
        attempts: 1,
        last_error: None,
    };
    let text = destination_label(&status);
    assert!(text.contains("stored"));
    assert!(!text.contains("delivered"));
    assert!(text.contains("100"));
}
#[test]
fn terminal_escape_sequences_render_as_text() {
    let mut state = AppState::default();
    state.screen = Screen::Article;
    state.detail = "safe\u{1b}]52;c;payload\u{07}\u{1b}[31m end".into();
    let text = screen_text(&state);
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains('\u{07}'));
    assert!(text.contains("\\u{001b}"));
}
#[test]
fn unicode_navigation_does_not_panic() {
    let mut state = AppState::default();
    state.screen = Screen::Article;
    state.detail = "漢😀é\n".repeat(100);
    state.reduce(Action::PageDown);
    state.reduce(Action::PageDown);
    assert!(screen_text(&state).contains('漢'));
}
#[test]
fn keyboard_form_is_cleared_on_escape() {
    let mut state = AppState::default();
    handle_key(&mut state, key('i'));
    assert!(state.form.is_some());
    handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(state.form.is_none());
    assert_eq!(state.screen, Screen::Profiles);
}
#[test]
fn failed_refresh_does_not_schedule_another_refresh() {
    let mut state = AppState::default();
    state.screen = Screen::Articles;
    let context = state.begin_task();
    assert!(!state.finish_task(context, Err(ClientError::Network)));
    assert!(!state.busy);
    assert!(state.status.contains("network"));
}
#[test]
fn outbox_enter_opens_relay_errors_and_retry_times_at_any_width() {
    let mut state = AppState::default();
    state.screen = Screen::Outbox;
    state.outbox = vec![OutboxEntry {
        event_id: event("queued", 100).event().id,
        profile_id: ProfileId::new_v4(),
        created_at: 100,
        destinations: vec![DestinationStatus {
            relay: "https://relay.example".into(),
            state: DestinationState::Refused,
            retain_until: None,
            retry_at: Some(500),
            attempts: 1,
            last_error: Some("policy rejected this event".into()),
        }],
    }];
    handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );
    assert_eq!(state.screen, Screen::Article);
    assert!(state.detail.contains("https://relay.example"));
    assert!(state.detail.contains("policy rejected this event"));
    assert!(state.detail.contains("500"));
}

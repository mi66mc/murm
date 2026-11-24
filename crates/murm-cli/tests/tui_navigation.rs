use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use murm_cli::tui::{
    Action, AppState, Screen, action_from_key,
    terminal::{TerminalControl, TerminalGuard},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[test]
fn starts_at_profiles_when_empty() {
    assert_eq!(AppState::default().screen, Screen::Profiles);
}
#[test]
fn navigation_bursts_work_while_background_task_is_busy() {
    let mut state = AppState::default();
    let context = state.begin_task();
    for _ in 0..50 {
        for c in ['2', '3', '4', '5', '1'] {
            murm_cli::tui::handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
    }
    murm_cli::tui::handle_key(
        &mut state,
        KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE),
    );
    assert_eq!(state.screen, Screen::Relays);
    assert!(state.apply_result(
        context,
        murm_cli::tui::UiMessage::Detail {
            title: "Late response".into(),
            text: "data".into()
        }
    ));
    assert_eq!(state.screen, Screen::Relays);
    assert!(!state.busy);
}
#[test]
fn key_release_does_not_navigate() {
    let mut state = AppState::default();
    let key = KeyEvent::new_with_kind(
        KeyCode::Char('3'),
        KeyModifiers::NONE,
        crossterm::event::KeyEventKind::Release,
    );
    murm_cli::tui::handle_key(&mut state, key);
    assert_eq!(state.screen, Screen::Profiles);
}
#[test]
fn help_blocks_hidden_profile_removal_and_escape_only_dismisses_help() {
    let mut state = AppState::default();
    state.profiles = vec![murm_cli::ProfileSummary {
        id: uuid::Uuid::new_v4(),
        name: "Test".into(),
        public_key: murm_protocol::derive_public_key(&[1; 32]),
        mode: murm_cli::ProfileMode::Signing,
        locked: true,
        relays: vec![],
    }];
    state.help = true;
    assert!(
        murm_cli::tui::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)
        )
        .is_none()
    );
    assert!(state.confirm.is_none());
    assert!(
        murm_cli::tui::handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        )
        .is_none()
    );
    murm_cli::tui::handle_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!state.help);
    assert_eq!(state.screen, Screen::Profiles);
}
#[test]
fn escape_restores_previous_screen() {
    let mut state = AppState::default();
    state.reduce(Action::Navigate(Screen::Drafts));
    state.reduce(Action::Navigate(Screen::Relays));
    state.reduce(Action::Back);
    assert_eq!(state.screen, Screen::Drafts);
}
#[test]
fn noninteractive_prints_help() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_murm-cli"))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
}
#[test]
fn ctrl_c_exits_cleanly() {
    let mut state = AppState::default();
    state
        .reduce(action_from_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)).unwrap());
    assert!(!state.running);
}
#[test]
fn page_keys_move_bounded_list_selection() {
    let profile = murm_cli::ProfileSummary {
        id: uuid::Uuid::new_v4(),
        name: "Reader".into(),
        public_key: murm_protocol::derive_public_key(&[1; 32]),
        mode: murm_cli::ProfileMode::ReadOnly,
        locked: false,
        relays: vec![],
    };
    let mut state = AppState::default();
    state.profiles = vec![profile; 30];
    state.reduce(Action::PageDown);
    assert_eq!(state.selected, 10);
    for _ in 0..5 {
        state.reduce(Action::PageDown);
    }
    assert_eq!(state.selected, 29);
    state.reduce(Action::Previous);
    assert_eq!(state.selected, 28);
    state.reduce(Action::PageUp);
    assert_eq!(state.selected, 18);
}
struct Control {
    restored: Arc<AtomicUsize>,
    fail: bool,
}
impl TerminalControl for Control {
    fn enter(&mut self) -> std::io::Result<()> {
        if self.fail {
            Err(std::io::Error::other("test"))
        } else {
            Ok(())
        }
    }
    fn restore(&mut self) -> std::io::Result<()> {
        self.restored.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
#[test]
fn terminal_restored_after_error() {
    let restored = Arc::new(AtomicUsize::new(0));
    let result = (|| -> std::io::Result<()> {
        let _guard = TerminalGuard::new(Control {
            restored: restored.clone(),
            fail: false,
        })?;
        Err(std::io::Error::other("test"))
    })();
    assert!(result.is_err());
    assert_eq!(restored.load(Ordering::SeqCst), 1);
    assert!(
        TerminalGuard::new(Control {
            restored: restored.clone(),
            fail: true
        })
        .is_err()
    );
    assert_eq!(restored.load(Ordering::SeqCst), 2);
}

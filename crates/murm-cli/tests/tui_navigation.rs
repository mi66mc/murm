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

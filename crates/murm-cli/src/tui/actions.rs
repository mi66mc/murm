use super::Screen;
use crossterm::event::{KeyCode,KeyEvent,KeyModifiers};
#[derive(Debug,Clone,Copy)]
pub enum Action{Navigate(Screen),Back,Next,Previous,PageDown,PageUp,Help,Quit}
pub fn action_from_key(key:KeyEvent)->Option<Action>{
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code==KeyCode::Char('c'){return Some(Action::Quit);}
    match key.code{
        KeyCode::Esc=>Some(Action::Back),KeyCode::Down=>Some(Action::Next),KeyCode::Up=>Some(Action::Previous),
        KeyCode::PageDown=>Some(Action::PageDown),KeyCode::PageUp=>Some(Action::PageUp),
        KeyCode::Char('?')=>Some(Action::Help),KeyCode::Char('q')=>Some(Action::Quit),
        KeyCode::Char('1')=>Some(Action::Navigate(Screen::Profiles)),KeyCode::Char('2')=>Some(Action::Navigate(Screen::Articles)),
        KeyCode::Char('3')=>Some(Action::Navigate(Screen::Drafts)),KeyCode::Char('4')=>Some(Action::Navigate(Screen::Relays)),
        KeyCode::Char('5')=>Some(Action::Navigate(Screen::Outbox)),_=>None,
    }
}

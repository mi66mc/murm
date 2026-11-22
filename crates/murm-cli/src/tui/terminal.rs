use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io;
pub trait TerminalControl {
    fn enter(&mut self) -> io::Result<()>;
    fn restore(&mut self) -> io::Result<()>;
}
pub struct TerminalGuard<C: TerminalControl> {
    control: C,
    active: bool,
}
impl<C: TerminalControl> TerminalGuard<C> {
    pub fn new(mut control: C) -> io::Result<Self> {
        if let Err(error) = control.enter() {
            let _ = control.restore();
            return Err(error);
        }
        Ok(Self {
            control,
            active: true,
        })
    }
    pub fn suspend(&mut self) -> io::Result<()> {
        if self.active {
            self.active = false;
            self.control.restore()?;
        }
        Ok(())
    }
    pub fn resume(&mut self) -> io::Result<()> {
        if !self.active {
            if let Err(error) = self.control.enter() {
                let _ = self.control.restore();
                return Err(error);
            }
            self.active = true;
        }
        Ok(())
    }
}
impl<C: TerminalControl> Drop for TerminalGuard<C> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.control.restore();
        }
    }
}
pub struct CrosstermControl;
fn restore() -> io::Result<()> {
    let raw = disable_raw_mode();
    let screen = execute!(
        io::stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
    raw.and(screen)
}
impl TerminalControl for CrosstermControl {
    fn enter(&mut self) -> io::Result<()> {
        static HOOK: std::sync::Once = std::sync::Once::new();
        HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let _ = restore();
                previous(info);
            }));
        });
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            Hide
        )
    }
    fn restore(&mut self) -> io::Result<()> {
        restore()
    }
}

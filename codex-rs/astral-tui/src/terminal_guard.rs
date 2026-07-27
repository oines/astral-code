use std::io;

use crossterm::event::DisableBracketedPaste;
use crossterm::event::DisableMouseCapture;
use crossterm::event::EnableBracketedPaste;
use crossterm::event::EnableMouseCapture;
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;

pub(crate) struct TerminalGuard {
    screen: Screen,
    active: bool,
}

#[derive(Clone, Copy)]
enum Screen {
    Inline,
    Alternate,
}

impl TerminalGuard {
    pub(crate) fn enter_inline() -> io::Result<Self> {
        Self::enter(Screen::Inline)
    }

    pub(crate) fn enter_alternate() -> io::Result<Self> {
        Self::enter(Screen::Alternate)
    }

    fn enter(screen: Screen) -> io::Result<Self> {
        match screen {
            Screen::Inline => execute!(std::io::stdout(), EnableBracketedPaste)?,
            Screen::Alternate => execute!(
                std::io::stdout(),
                EnterAlternateScreen,
                EnableBracketedPaste,
                EnableMouseCapture
            )?,
        }
        if let Err(error) = enable_raw_mode() {
            match screen {
                Screen::Inline => {
                    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
                }
                Screen::Alternate => {
                    let _ = execute!(
                        std::io::stdout(),
                        DisableMouseCapture,
                        DisableBracketedPaste,
                        LeaveAlternateScreen
                    );
                }
            }
            return Err(error);
        }
        Ok(Self {
            screen,
            active: true,
        })
    }

    pub(crate) fn restore(&mut self) {
        if !self.active {
            return;
        }
        let _ = disable_raw_mode();
        match self.screen {
            Screen::Inline => {
                let _ = execute!(std::io::stdout(), DisableBracketedPaste);
            }
            Screen::Alternate => {
                let _ = execute!(
                    std::io::stdout(),
                    DisableMouseCapture,
                    DisableBracketedPaste,
                    LeaveAlternateScreen
                );
            }
        }
        self.active = false;
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

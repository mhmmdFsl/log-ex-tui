use std::io;
use std::time::Duration;

use crossterm::cursor::Show;
use crossterm::event::{poll, read, Event, KeyEvent};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use crate::app::Message;

pub type Term = Terminal<CrosstermBackend<io::Stdout>>;

pub fn init() -> io::Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.hide_cursor()?;
    Ok(terminal)
}

pub fn restore() -> io::Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen, Show)?;
    disable_raw_mode()?;
    Ok(())
}

pub fn set_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        restore().ok();
        hook(panic);
    }));
}

pub fn event_stream(tx: mpsc::UnboundedSender<Message>) {
    std::thread::spawn(move || {
        loop {
            if let Ok(true) = poll(Duration::from_millis(100)) {
                if let Ok(event) = read() {
                    match event {
                        Event::Key(KeyEvent {
                            code, modifiers, ..
                        }) => {
                            let msg = Message::Key { code, modifiers };
                            if tx.send(msg).is_err() {
                                break;
                            }
                        }
                        Event::Resize(..) => {
                            let msg = Message::Resize;
                            if tx.send(msg).is_err() {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            // tick signal for periodic work
            if tx.send(Message::Tick).is_err() {
                break;
            }
        }
    });
}

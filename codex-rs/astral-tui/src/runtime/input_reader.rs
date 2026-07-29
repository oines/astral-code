//! Cancellation-safe terminal input pump.
//!
//! Grok Build reads crossterm events on a dedicated thread because repeatedly
//! dropping `EventStream::next()` inside `select!` can strand its background
//! waker. Astral's scroll clock adds frequent competing wakeups, so it uses
//! the same ownership boundary and lets the async loop receive from a channel.

use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::Duration;

use crossterm::event::Event;
use tokio::sync::mpsc;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(super) struct TerminalEventReader {
    receiver: mpsc::UnboundedReceiver<io::Result<Event>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TerminalEventReader {
    pub(super) fn start() -> io::Result<Self> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("astral-terminal-input".to_string())
            .spawn(move || {
                while !reader_stop.load(Ordering::Acquire) {
                    match crossterm::event::poll(INPUT_POLL_INTERVAL) {
                        Ok(true) => {
                            let event = crossterm::event::read();
                            let failed = event.is_err();
                            if sender.send(event).is_err() || failed {
                                break;
                            }
                        }
                        Ok(false) => {}
                        Err(error) => {
                            let _ = sender.send(Err(error));
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            receiver,
            stop,
            thread: Some(thread),
        })
    }

    pub(super) async fn recv(&mut self) -> Option<io::Result<Event>> {
        self.receiver.recv().await
    }

    pub(super) fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for TerminalEventReader {
    fn drop(&mut self) {
        self.stop();
    }
}

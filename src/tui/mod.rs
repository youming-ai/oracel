pub mod state;
pub mod ui;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, KeyCode};
use tokio::sync::RwLock;

use state::TuiState;

/// Run the TUI event loop. Blocks until user presses 'q'.
/// Takes a tokio RwLock so it can be shared with async bot tasks.
pub fn run(
    app_state: Arc<RwLock<TuiState>>,
    shutdown: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut terminal = ratatui::init();
    let result = (|| {
        let tick_rate = Duration::from_millis(250);
        let mut last_tick = std::time::Instant::now();

        while !shutdown.load(Ordering::Acquire) {
            terminal.draw(|frame| {
                let state = app_state.try_read();
                ui::render(frame, state.ok().as_deref());
            })?;

            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or(Duration::ZERO);
            if crossterm::event::poll(timeout)? {
                if let Event::Key(key) = crossterm::event::read()? {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            shutdown.store(true, Ordering::Release);
                            break;
                        }
                        KeyCode::Up => {
                            if let Ok(mut state) = app_state.try_write() {
                                state.scroll_offset = state.scroll_offset.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            if let Ok(mut state) = app_state.try_write() {
                                let max_offset = state.recent_trades.len().saturating_sub(1);
                                state.scroll_offset = (state.scroll_offset + 1).min(max_offset);
                            }
                        }
                        _ => {}
                    }
                }
            }

            if last_tick.elapsed() >= tick_rate {
                last_tick = std::time::Instant::now();
            }
        }

        Ok(())
    })();
    ratatui::restore();
    result
}

//! `tui::terminal` — panic-safe wrapper around `ratatui::Terminal`.
//!
//! Owns raw-mode entry/exit, a panic hook that restores terminal state
//! before propagating, and the inline viewport that pins the bottom N rows
//! for chrome while leaving normal scrollback intact above.
//!
//! Slice 2 sub-phase: struct + Drop guard land here; full initialization
//! wires in when the event loop is built out.

use std::io::{self, Stdout, Write};
use std::sync::Once;

use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::{Terminal, TerminalOptions, Viewport};

/// Rows reserved at the bottom of the terminal for pinned chrome (input
/// box + helper line + status bar; grows when a mode chip or overlay is
/// visible). The message stream renders above this region via normal
/// stdout.
pub const INLINE_VIEWPORT_HEIGHT: u16 = 8;

const _: () = assert!(
    INLINE_VIEWPORT_HEIGHT >= 5 && INLINE_VIEWPORT_HEIGHT <= 12,
    "inline viewport height must fit input+helper+status without squeezing the message stream"
);

/// Terminal owner. Drops raw mode + shows cursor when it goes out of scope,
/// and installs a one-shot panic hook that does the same before propagating
/// the panic. Callers get frames via [`Tui::terminal_mut`].
pub struct Tui {
    inner: Terminal<CrosstermBackend<Stdout>>,
    /// Currently active inline-viewport height. Changed by
    /// [`Tui::resize_viewport`] when overlays need more room than the
    /// default chrome region.
    viewport_height: u16,
}

impl Tui {
    /// Enter raw mode, install the panic-restore hook (once per process),
    /// and construct the inline-viewport terminal. Returns an error if
    /// raw-mode entry fails.
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from `enable_raw_mode` or terminal
    /// construction.
    pub fn new() -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        let backend = CrosstermBackend::new(io::stdout());
        let inner = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(INLINE_VIEWPORT_HEIGHT),
            },
        )?;
        Ok(Self {
            inner,
            viewport_height: INLINE_VIEWPORT_HEIGHT,
        })
    }

    /// Escape hatch for callers that need to draw a frame. Prefer higher-
    /// level `render`/`narrate` methods once they exist.
    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.inner
    }

    /// Current inline-viewport height. Callers use this to decide
    /// whether to call [`Tui::resize_viewport`] before drawing.
    #[must_use]
    pub fn viewport_height(&self) -> u16 {
        self.viewport_height
    }

    /// Temporarily leave raw mode + show cursor so a caller can print
    /// a scrolling animation directly to stdout (e.g. the Ctrl+E
    /// glitch transition). Call [`Tui::resume`] afterwards to
    /// re-enter raw mode and force a full ratatui redraw.
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from `disable_raw_mode` or the
    /// cursor-show escape.
    pub fn suspend(&self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(io::stdout(), crossterm::cursor::Show)?;
        io::stdout().flush()?;
        Ok(())
    }

    /// Re-enter raw mode + hide cursor + rebuild the ratatui Terminal
    /// so its inline viewport re-anchors to the CURRENT cursor
    /// position (which the caller shifted during suspended I/O). Just
    /// calling `terminal.clear()` here leaves ratatui thinking the
    /// viewport is where it was before suspend, which is wrong after
    /// the animation scrolled the terminal.
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from `enable_raw_mode`, the cursor
    /// escape, or terminal construction.
    pub fn resume(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(io::stdout(), crossterm::cursor::Hide)?;
        let backend = CrosstermBackend::new(io::stdout());
        self.inner = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(self.viewport_height),
            },
        )?;
        Ok(())
    }

    /// Rebuild the inner `Terminal` with a new inline-viewport height.
    /// Ratatui's inline viewport is fixed at construction, so growing
    /// it (e.g. when the model / effort / slash-menu overlays open)
    /// requires swapping out the terminal handle. Raw mode is
    /// unaffected — only the ratatui bookkeeping resets.
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from `Terminal::with_options` or the
    /// initial clear.
    pub fn resize_viewport(&mut self, height: u16) -> io::Result<()> {
        if height == self.viewport_height {
            return Ok(());
        }
        // Clear the current viewport area before we rebuild so we don't
        // leave a ghost of the previous chrome above the new one.
        let _ = self.inner.clear();
        let backend = CrosstermBackend::new(io::stdout());
        self.inner = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        )?;
        self.viewport_height = height;
        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Best-effort restore — errors are swallowed because Drop can't
        // return them and we're already unwinding OR shutting down.
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            crossterm::cursor::Show,
            crossterm::terminal::LeaveAlternateScreen
        );
        let _ = io::stdout().flush();
    }
}

/// Installs a panic hook that restores the terminal (raw-mode off, cursor
/// visible, alternate screen left) before delegating to the previously
/// installed hook. Idempotent — safe to call multiple times.
fn install_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let prior = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                crossterm::cursor::Show,
                crossterm::terminal::LeaveAlternateScreen
            );
            let _ = io::stdout().flush();
            prior(info);
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sanity that the viewport height stays reasonable is enforced at
    // compile time via `const _: () = assert!(...)` above.

    #[test]
    fn panic_hook_is_idempotent() {
        // Repeated calls must not deadlock or replace the hook chain
        // multiple times.
        install_panic_hook();
        install_panic_hook();
        install_panic_hook();
    }
}

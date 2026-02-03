//! Terminal progress display for spawn chunk generation.
//!
//! Shows a colored ANSI grid with real-time chunk generation progress.
//! The [`SwitchableWriter`] replaces the default tracing writer so that
//! log lines appear above the grid without disturbing it.

use std::io::{self, Write};
use std::sync::Arc;

use steel_core::chunk::chunk_access::ChunkStatus;
use steel_utils::locks::SyncMutex;
use tracing_subscriber::fmt::MakeWriter;

use super::DISPLAY_DIAMETER;

/// Grid type alias for convenience.
pub type Grid = [[Option<ChunkStatus>; DISPLAY_DIAMETER]; DISPLAY_DIAMETER];

// ---------------------------------------------------------------------------
// SwitchableWriter
// ---------------------------------------------------------------------------

/// A tracing writer that can redirect output through a [`SpawnProgressDisplay`].
///
/// When the display is not activated, output goes directly to stderr.
/// When activated, log lines are rendered above the progress grid.
///
/// Internally reference-counted — cloning is cheap and shares the same state.
#[derive(Clone)]
pub struct SwitchableWriter {
    inner: Arc<SyncMutex<Option<SpawnProgressDisplay>>>,
}

impl Default for SwitchableWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl SwitchableWriter {
    /// Creates a new writer in normal (stderr) mode.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SyncMutex::new(None)),
        }
    }

    /// Activates the progress display. Log output will be routed through it.
    pub fn activate(&self) {
        *self.inner.lock() = Some(SpawnProgressDisplay::new());
    }

    /// Deactivates the progress display, erasing the grid from the terminal.
    pub fn deactivate(&self) {
        if let Some(mut display) = self.inner.lock().take() {
            display.erase_final();
        }
    }

    /// Updates the internal grid state (always) and re-renders if requested.
    pub fn update_grid(&self, grid: &Grid, render: bool) {
        if let Some(display) = self.inner.lock().as_mut() {
            display.set_grid(grid);
            if render {
                display.render_current();
            }
        }
    }
}

impl<'a> MakeWriter<'a> for SwitchableWriter {
    type Writer = SwitchableWriteTarget;

    fn make_writer(&'a self) -> Self::Writer {
        SwitchableWriteTarget {
            inner: Arc::clone(&self.inner),
            buffer: Vec::with_capacity(256),
        }
    }
}

/// Per-log-event writer that buffers the formatted line and flushes on drop.
pub struct SwitchableWriteTarget {
    inner: Arc<SyncMutex<Option<SpawnProgressDisplay>>>,
    buffer: Vec<u8>,
}

impl Write for SwitchableWriteTarget {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for SwitchableWriteTarget {
    fn drop(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let mut inner = self.inner.lock();
        if let Some(display) = inner.as_mut() {
            display.write_log_line(&self.buffer);
        } else {
            drop(inner);
            let _ = io::stderr().write_all(&self.buffer);
        }
    }
}

// ---------------------------------------------------------------------------
// SpawnProgressDisplay
// ---------------------------------------------------------------------------

/// Returns the vanilla RGB color for a chunk status.
/// Colors are taken from `LevelLoadingScreen.COLORS` in the vanilla client.
const fn status_color(status: Option<ChunkStatus>) -> (u8, u8, u8) {
    match status {
        None | Some(ChunkStatus::Empty) => (84, 84, 84),
        Some(ChunkStatus::StructureStarts) => (153, 153, 153),
        Some(ChunkStatus::StructureReferences) => (95, 97, 145),
        Some(ChunkStatus::Biomes) => (128, 178, 82),
        Some(ChunkStatus::Noise) => (209, 209, 209),
        Some(ChunkStatus::Surface) => (114, 104, 9),
        Some(ChunkStatus::Carvers) => (48, 53, 114),
        Some(ChunkStatus::Features) => (33, 198, 0),
        Some(ChunkStatus::InitializeLight) => (204, 204, 204),
        Some(ChunkStatus::Light) => (255, 224, 160),
        Some(ChunkStatus::Spawn) => (242, 96, 96),
        Some(ChunkStatus::Full) => (255, 255, 255),
    }
}

/// Terminal progress display showing a colored grid of chunk generation statuses.
struct SpawnProgressDisplay {
    grid: Grid,
    rendered: bool,
}

impl SpawnProgressDisplay {
    /// Creates a new display with all cells unloaded (black).
    const fn new() -> Self {
        Self {
            grid: [[None; DISPLAY_DIAMETER]; DISPLAY_DIAMETER],
            rendered: false,
        }
    }

    /// Erases the grid from the terminal by moving the cursor up and clearing lines.
    fn erase(&self, out: &mut impl Write) {
        if !self.rendered {
            return;
        }
        let term_lines = DISPLAY_DIAMETER.div_ceil(2);
        for _ in 0..term_lines {
            let _ = write!(out, "\x1b[1A\x1b[2K");
        }
    }

    /// Renders the grid to the given writer (appends new lines).
    /// Uses half-block characters to render 2 rows per terminal line.
    fn render(&self, out: &mut impl Write) {
        for z in (0..DISPLAY_DIAMETER).step_by(2) {
            for x in 0..DISPLAY_DIAMETER {
                let (tr, tg, tb) = status_color(self.grid[z][x]);
                // ▀ = upper half block: foreground is top row, background is bottom row
                if z + 1 < DISPLAY_DIAMETER {
                    let (br, bg, bb) = status_color(self.grid[z + 1][x]);
                    let _ = write!(out, "\x1b[38;2;{tr};{tg};{tb}m\x1b[48;2;{br};{bg};{bb}m▀");
                } else {
                    // Last row with odd diameter: use default background (transparent)
                    let _ = write!(out, "\x1b[38;2;{tr};{tg};{tb}m▀");
                }
            }
            let _ = writeln!(out, "\x1b[0m");
        }
    }

    /// Overwrites the grid in-place (moves cursor up, rewrites each line).
    /// Uses half-block characters to render 2 rows per terminal line.
    fn render_overwrite(&self, out: &mut impl Write) {
        let term_lines = DISPLAY_DIAMETER.div_ceil(2);
        let _ = write!(out, "\x1b[{term_lines}A");
        for z in (0..DISPLAY_DIAMETER).step_by(2) {
            let _ = write!(out, "\r");
            for x in 0..DISPLAY_DIAMETER {
                let (tr, tg, tb) = status_color(self.grid[z][x]);
                if z + 1 < DISPLAY_DIAMETER {
                    let (br, bg, bb) = status_color(self.grid[z + 1][x]);
                    let _ = write!(out, "\x1b[38;2;{tr};{tg};{tb}m\x1b[48;2;{br};{bg};{bb}m▀");
                } else {
                    let _ = write!(out, "\x1b[38;2;{tr};{tg};{tb}m▀");
                }
            }
            let _ = writeln!(out, "\x1b[0m\x1b[K");
        }
    }

    /// Updates the internal grid state.
    const fn set_grid(&mut self, new_grid: &Grid) {
        self.grid = *new_grid;
    }

    /// Renders the current grid state to the terminal.
    fn render_current(&mut self) {
        let mut out = io::stderr().lock();
        if self.rendered {
            self.render_overwrite(&mut out);
        } else {
            self.render(&mut out);
        }
        let _ = out.flush();
        self.rendered = true;
    }

    /// Erases the grid, writes a log line, then re-renders the grid.
    fn write_log_line(&mut self, line: &[u8]) {
        let mut out = io::stderr().lock();
        self.erase(&mut out);
        let _ = out.write_all(line);
        self.render(&mut out);
        let _ = out.flush();
        self.rendered = true;
    }

    /// Fully erases the grid from the terminal (for cleanup).
    fn erase_final(&mut self) {
        if !self.rendered {
            return;
        }
        let mut out = io::stderr().lock();
        self.erase(&mut out);
        let _ = out.flush();
        self.rendered = false;
    }
}

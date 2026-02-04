//! Terminal progress display for spawn chunk generation.
//!
//! Shows a colored ANSI grid with real-time chunk generation progress.

use crate::logger::output::Output;
use crate::spawn_progress::{DISPLAY_DIAMETER, DISPLAY_RADIUS};
use crossterm::style::Color;
use crossterm::{
    cursor::{MoveRight, MoveUp},
    style::{Color::Rgb, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::io::{Result, Write};
use steel_core::chunk::chunk_access::ChunkStatus;

/// Grid type alias for convenience.
pub type Grid = [[Option<ChunkStatus>; DISPLAY_DIAMETER]; DISPLAY_DIAMETER];

// ---------------------------------------------------------------------------
// SpawnProgressDisplay
// ---------------------------------------------------------------------------

/// Returns the vanilla RGB color for a chunk status.
/// Colors are taken from `LevelLoadingScreen.COLORS` in the vanilla client.
const fn status_color(status: Option<ChunkStatus>) -> Color {
    match status {
        None | Some(ChunkStatus::Empty) => Rgb {
            r: 84,
            g: 84,
            b: 84,
        },
        Some(ChunkStatus::StructureStarts) => Rgb {
            r: 153,
            g: 153,
            b: 153,
        },
        Some(ChunkStatus::StructureReferences) => Rgb {
            r: 95,
            g: 97,
            b: 145,
        },
        Some(ChunkStatus::Biomes) => Rgb {
            r: 128,
            g: 178,
            b: 82,
        },
        Some(ChunkStatus::Noise) => Rgb {
            r: 209,
            g: 209,
            b: 209,
        },
        Some(ChunkStatus::Surface) => Rgb {
            r: 114,
            g: 104,
            b: 9,
        },
        Some(ChunkStatus::Carvers) => Rgb {
            r: 48,
            g: 53,
            b: 114,
        },
        Some(ChunkStatus::Features) => Rgb {
            r: 33,
            g: 198,
            b: 0,
        },
        Some(ChunkStatus::InitializeLight) => Rgb {
            r: 204,
            g: 204,
            b: 204,
        },
        Some(ChunkStatus::Light) => Rgb {
            r: 255,
            g: 224,
            b: 160,
        },
        Some(ChunkStatus::Spawn) => Rgb {
            r: 242,
            g: 96,
            b: 96,
        },
        Some(ChunkStatus::Full) => Rgb {
            r: 255,
            g: 255,
            b: 255,
        },
    }
}

/// Terminal progress display showing a colored grid of chunk generation statuses.
pub struct SpawnProgressDisplay {
    grid: Grid,
    /// If the progress is being displayed
    pub rendered: bool,
}

impl SpawnProgressDisplay {
    /// Creates a new display with all cells unloaded (black).
    pub const fn new() -> Self {
        Self {
            grid: [[None; DISPLAY_DIAMETER]; DISPLAY_DIAMETER],
            rendered: false,
        }
    }

    /// Updates the internal grid state.
    pub const fn set_grid(&mut self, new_grid: &Grid) {
        self.grid = *new_grid;
    }

    pub fn rewrite(&self, out: &mut Output) -> Result<()> {
        write!(
            out,
            "{}\n{}",
            MoveUp(DISPLAY_RADIUS as u16 + 2),
            Clear(ClearType::FromCursorDown)
        )?;
        let w = if let Ok((w, _)) = terminal::size() {
            w / 2 - DISPLAY_RADIUS as u16 - 1
        } else {
            0
        };
        for z in (0..DISPLAY_DIAMETER).step_by(2) {
            write!(out, "\r")?;
            if w != 0 {
                write!(out, "{}", MoveRight(w))?;
            }
            for x in 0..DISPLAY_DIAMETER {
                let front = status_color(self.grid[z][x]);
                if z + 1 < DISPLAY_DIAMETER {
                    let back = status_color(self.grid[z + 1][x]);
                    write!(
                        out,
                        "{}{}▀",
                        SetForegroundColor(front),
                        SetBackgroundColor(back)
                    )?;
                } else {
                    write!(out, "{}▀", SetForegroundColor(front))?;
                }
            }
            writeln!(out, "{ResetColor}")?;
            out.flush()?;
        }
        write!(out, "\r")?;
        out.cursor_to((0, 0), out.get_current_pos())
    }
}

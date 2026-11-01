//! Scoreboard display packets: objectives, display slots and scores.
//!
//! Field order follows vanilla `ClientboundSetObjectivePacket`,
//! `ClientboundSetDisplayObjectivePacket`, `ClientboundSetScorePacket` and
//! `ClientboundResetScorePacket`.

use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::packets::play::{
    C_RESET_SCORE, C_SET_DISPLAY_OBJECTIVE, C_SET_OBJECTIVE, C_SET_SCORE,
};
use steel_utils::{codec::VarInt, serial::WriteTo};
use text_components::TextComponent;

/// How a score's number is rendered (vanilla `NumberFormat`, registry order blank, styled, fixed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NumberFormat {
    /// Hide the number.
    Blank,
    /// Show this text instead of the number.
    Fixed(Box<TextComponent>),
}

impl WriteTo for NumberFormat {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        match self {
            Self::Blank => VarInt(0).write(writer),
            Self::Fixed(text) => {
                VarInt(2).write(writer)?;
                text.write(writer)
            }
        }
    }
}

fn write_string(value: &str, writer: &mut impl Write) -> Result<()> {
    let length = i32::try_from(value.len()).map_err(std::io::Error::other)?;
    VarInt(length).write(writer)?;
    writer.write_all(value.as_bytes())
}

fn write_optional<T: WriteTo>(value: Option<&T>, writer: &mut impl Write) -> Result<()> {
    match value {
        Some(value) => {
            true.write(writer)?;
            value.write(writer)
        }
        None => false.write(writer),
    }
}

/// What a [`CSetObjective`] does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectiveMethod {
    /// Creates the objective.
    Add {
        /// Display name.
        display_name: TextComponent,
        /// Default number format of its scores.
        number_format: Option<NumberFormat>,
    },
    /// Removes the objective.
    Remove,
    /// Changes the display name and format.
    Change {
        /// Display name.
        display_name: TextComponent,
        /// Default number format of its scores.
        number_format: Option<NumberFormat>,
    },
}

/// Adds, removes or changes an objective (always rendered as integers).
#[derive(ClientPacket, Debug, Clone)]
#[packet_id(Play = C_SET_OBJECTIVE)]
pub struct CSetObjective {
    /// Objective name.
    pub name: String,
    /// Operation.
    pub method: ObjectiveMethod,
}

impl WriteTo for CSetObjective {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_string(&self.name, writer)?;
        let (method, data) = match &self.method {
            ObjectiveMethod::Add {
                display_name,
                number_format,
            } => (0_i8, Some((display_name, number_format))),
            ObjectiveMethod::Remove => (1, None),
            ObjectiveMethod::Change {
                display_name,
                number_format,
            } => (2, Some((display_name, number_format))),
        };
        method.write(writer)?;
        if let Some((display_name, number_format)) = data {
            display_name.write(writer)?;
            // Render type INTEGER.
            VarInt(0).write(writer)?;
            write_optional(number_format.as_ref(), writer)?;
        }
        Ok(())
    }
}

/// Vanilla display slots (team-colored sidebars are not used).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplaySlot {
    /// Tab list.
    List = 0,
    /// Right-side sidebar.
    Sidebar = 1,
    /// Below player names.
    BelowName = 2,
}

/// Shows an objective in a display slot; an empty name clears the slot.
#[derive(ClientPacket, Debug, Clone)]
#[packet_id(Play = C_SET_DISPLAY_OBJECTIVE)]
pub struct CSetDisplayObjective {
    /// Display slot.
    pub slot: DisplaySlot,
    /// Objective name, or empty.
    pub objective: String,
}

impl WriteTo for CSetDisplayObjective {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.slot as i32).write(writer)?;
        write_string(&self.objective, writer)
    }
}

/// Sets one score line.
#[derive(ClientPacket, Debug, Clone)]
#[packet_id(Play = C_SET_SCORE)]
pub struct CSetScore {
    /// Score holder name (unique per line).
    pub owner: String,
    /// Objective name.
    pub objective: String,
    /// Score; sidebars sort by it, highest first.
    pub score: i32,
    /// Text shown instead of the holder name.
    pub display: Option<TextComponent>,
    /// Number format override.
    pub number_format: Option<NumberFormat>,
}

impl WriteTo for CSetScore {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_string(&self.owner, writer)?;
        write_string(&self.objective, writer)?;
        VarInt(self.score).write(writer)?;
        write_optional(self.display.as_ref(), writer)?;
        write_optional(self.number_format.as_ref(), writer)
    }
}

/// Removes a score holder from one objective, or from all when `objective` is `None`.
#[derive(ClientPacket, Debug, Clone)]
#[packet_id(Play = C_RESET_SCORE)]
pub struct CResetScore {
    /// Score holder name.
    pub owner: String,
    /// Objective name.
    pub objective: Option<String>,
}

impl WriteTo for CResetScore {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        write_string(&self.owner, writer)?;
        match &self.objective {
            Some(objective) => {
                true.write(writer)?;
                write_string(objective, writer)
            }
            None => false.write(writer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(packet: &impl WriteTo) -> Vec<u8> {
        let mut out = Vec::new();
        packet
            .write(&mut out)
            .unwrap_or_else(|error| panic!("packet should encode: {error}"));
        out
    }

    #[test]
    fn display_and_reset_follow_vanilla_field_order() {
        let display = CSetDisplayObjective {
            slot: DisplaySlot::Sidebar,
            objective: "sb".into(),
        };
        assert_eq!(bytes(&display), [1, 2, b's', b'b']);
        let reset = CResetScore {
            owner: "a".into(),
            objective: Some("sb".into()),
        };
        assert_eq!(bytes(&reset), [1, b'a', 1, 2, b's', b'b']);
        let reset_all = CResetScore {
            owner: "a".into(),
            objective: None,
        };
        assert_eq!(bytes(&reset_all), [1, b'a', 0]);
    }

    #[test]
    fn remove_objective_has_no_payload_and_scores_end_with_format() {
        let remove = CSetObjective {
            name: "sb".into(),
            method: ObjectiveMethod::Remove,
        };
        assert_eq!(bytes(&remove), [2, b's', b'b', 1]);
        let score = CSetScore {
            owner: "l".into(),
            objective: "sb".into(),
            score: 300,
            display: None,
            number_format: Some(NumberFormat::Blank),
        };
        // owner, objective, VarInt 300 = [0xAC, 0x02], no display, present blank format (type 0).
        assert_eq!(bytes(&score), [1, b'l', 2, b's', b'b', 0xAC, 0x02, 0, 1, 0]);
    }
}

//! Per-player HUD: a private sidebar, boss bars and a tab list header/footer override.
//!
//! Vanilla drives the sidebar from the shared server scoreboard and boss bars from
//! `ServerBossEvent` objects that track a set of players. Steel has no server scoreboard yet,
//! so this module keeps one private sidebar objective per client instead: each line is a
//! hidden score holder (`#l0`, `#l1`, ...) with a fixed descending score, a blank number format
//! and the line text as its display name, which is how Paper sidebar libraries render lines.
//! State lives on the [`PlayerSession`](super::PlayerSession) because the client keeps it
//! across respawns. Every update sends only what changed.

use rustc_hash::FxHashMap;
use steel_protocol::packets::game::{
    BossBarColor, BossBarOverlay, BossEventOperation, CBossEvent, CResetScore,
    CSetDisplayObjective, CSetObjective, CSetScore, CTabList, DisplaySlot, NumberFormat,
    ObjectiveMethod,
};
use text_components::TextComponent;
use uuid::Uuid;

use crate::player::Player;

/// Name of the private sidebar objective.
const SIDEBAR_OBJECTIVE: &str = "steel_sidebar";
/// Lines the vanilla client renders in a sidebar.
pub const MAX_SIDEBAR_LINES: usize = 15;

/// A boss bar as shown to one player.
#[derive(Debug, Clone, PartialEq)]
pub struct BossBar {
    /// Title.
    pub title: TextComponent,
    /// Fill, clamped to 0..=1.
    pub progress: f32,
    /// Color.
    pub color: BossBarColor,
    /// Segment style.
    pub overlay: BossBarOverlay,
    /// Vanilla flag bits: 1 darken screen, 2 boss music, 4 fog.
    pub flags: u8,
}

/// One HUD packet; kept as data so the diffing is testable without a connection.
#[derive(Debug, Clone)]
pub(crate) enum HudPacket {
    Objective(CSetObjective),
    Display(CSetDisplayObjective),
    Score(CSetScore),
    Reset(CResetScore),
    Boss(CBossEvent),
}

#[derive(Debug, Clone)]
struct Sidebar {
    title: TextComponent,
    lines: Vec<TextComponent>,
}

/// HUD state of one client connection.
#[derive(Debug, Default)]
pub struct PlayerHud {
    sidebar: Option<Sidebar>,
    boss_bars: FxHashMap<Uuid, BossBar>,
    tab_list: Option<(TextComponent, TextComponent)>,
}

fn line_owner(index: usize) -> String {
    format!("#l{index}")
}

impl PlayerHud {
    /// Shows or updates the sidebar; lines past [`MAX_SIDEBAR_LINES`] are dropped.
    pub(crate) fn set_sidebar(
        &mut self,
        title: TextComponent,
        lines: &[TextComponent],
    ) -> Vec<HudPacket> {
        let lines = &lines[..lines.len().min(MAX_SIDEBAR_LINES)];
        let mut packets = Vec::new();
        let previous = match self.sidebar.take() {
            None => {
                packets.push(HudPacket::Objective(CSetObjective {
                    name: SIDEBAR_OBJECTIVE.into(),
                    method: ObjectiveMethod::Add {
                        display_name: title.clone(),
                        number_format: Some(NumberFormat::Blank),
                    },
                }));
                packets.push(HudPacket::Display(CSetDisplayObjective {
                    slot: DisplaySlot::Sidebar,
                    objective: SIDEBAR_OBJECTIVE.into(),
                }));
                Vec::new()
            }
            Some(previous) => {
                if previous.title != title {
                    packets.push(HudPacket::Objective(CSetObjective {
                        name: SIDEBAR_OBJECTIVE.into(),
                        method: ObjectiveMethod::Change {
                            display_name: title.clone(),
                            number_format: Some(NumberFormat::Blank),
                        },
                    }));
                }
                previous.lines
            }
        };
        for (index, line) in lines.iter().enumerate() {
            if previous.get(index) == Some(line) {
                continue;
            }
            let Ok(score) = i32::try_from(MAX_SIDEBAR_LINES - index) else {
                continue;
            };
            packets.push(HudPacket::Score(CSetScore {
                owner: line_owner(index),
                objective: SIDEBAR_OBJECTIVE.into(),
                score,
                display: Some(line.clone()),
                number_format: None,
            }));
        }
        for index in lines.len()..previous.len() {
            packets.push(HudPacket::Reset(CResetScore {
                owner: line_owner(index),
                objective: Some(SIDEBAR_OBJECTIVE.into()),
            }));
        }
        self.sidebar = Some(Sidebar {
            title,
            lines: lines.to_vec(),
        });
        packets
    }

    /// Removes the sidebar.
    pub(crate) fn clear_sidebar(&mut self) -> Vec<HudPacket> {
        if self.sidebar.take().is_none() {
            return Vec::new();
        }
        vec![HudPacket::Objective(CSetObjective {
            name: SIDEBAR_OBJECTIVE.into(),
            method: ObjectiveMethod::Remove,
        })]
    }

    /// Shows or updates a boss bar.
    pub(crate) fn set_boss_bar(&mut self, id: Uuid, mut bar: BossBar) -> Vec<HudPacket> {
        bar.progress = bar.progress.clamp(0.0, 1.0);
        let boss = |operation| HudPacket::Boss(CBossEvent { id, operation });
        let Some(previous) = self.boss_bars.insert(id, bar.clone()) else {
            return vec![boss(BossEventOperation::Add {
                title: bar.title,
                progress: bar.progress,
                color: bar.color,
                overlay: bar.overlay,
                flags: bar.flags,
            })];
        };
        let mut packets = Vec::new();
        if previous.title != bar.title {
            packets.push(boss(BossEventOperation::UpdateName(bar.title)));
        }
        if previous.progress.to_bits() != bar.progress.to_bits() {
            packets.push(boss(BossEventOperation::UpdateProgress(bar.progress)));
        }
        if previous.color != bar.color || previous.overlay != bar.overlay {
            packets.push(boss(BossEventOperation::UpdateStyle(
                bar.color,
                bar.overlay,
            )));
        }
        if previous.flags != bar.flags {
            packets.push(boss(BossEventOperation::UpdateProperties(bar.flags)));
        }
        packets
    }

    /// Removes a boss bar.
    pub(crate) fn remove_boss_bar(&mut self, id: Uuid) -> Vec<HudPacket> {
        if self.boss_bars.remove(&id).is_none() {
            return Vec::new();
        }
        vec![HudPacket::Boss(CBossEvent {
            id,
            operation: BossEventOperation::Remove,
        })]
    }

    /// Whether a extension override replaces the server's tab list header and footer.
    pub(crate) const fn has_tab_list_override(&self) -> bool {
        self.tab_list.is_some()
    }
}

impl Player {
    fn send_hud(&self, packets: Vec<HudPacket>) {
        for packet in packets {
            match packet {
                HudPacket::Objective(packet) => self.send_packet(packet),
                HudPacket::Display(packet) => self.send_packet(packet),
                HudPacket::Score(packet) => self.send_packet(packet),
                HudPacket::Reset(packet) => self.send_packet(packet),
                HudPacket::Boss(packet) => self.send_packet(packet),
            }
        }
    }

    /// Shows or updates this player's private sidebar (top line first).
    pub fn set_sidebar(&self, title: TextComponent, lines: &[TextComponent]) {
        let packets = self.session.hud.lock().set_sidebar(title, lines);
        self.send_hud(packets);
    }

    /// Removes this player's sidebar.
    pub fn clear_sidebar(&self) {
        let packets = self.session.hud.lock().clear_sidebar();
        self.send_hud(packets);
    }

    /// Shows or updates a boss bar; only changed fields are sent.
    pub fn set_boss_bar(&self, id: Uuid, bar: BossBar) {
        let packets = self.session.hud.lock().set_boss_bar(id, bar);
        self.send_hud(packets);
    }

    /// Removes a boss bar.
    pub fn remove_boss_bar(&self, id: Uuid) {
        let packets = self.session.hud.lock().remove_boss_bar(id);
        self.send_hud(packets);
    }

    /// Replaces the tab list header and footer for this player, suppressing the server's
    /// periodic TPS footer until [`Player::clear_tab_list_override`].
    pub fn set_tab_list_override(&self, header: TextComponent, footer: TextComponent) {
        self.send_packet(CTabList::new(&header, &footer, self));
        self.session.hud.lock().tab_list = Some((header, footer));
    }

    /// Returns the tab list to the server's default; it refreshes on the next broadcast.
    pub fn clear_tab_list_override(&self) {
        self.session.hud.lock().tab_list = None;
    }

    /// Whether a extension override replaces this player's tab list header and footer.
    #[must_use]
    pub fn has_tab_list_override(&self) -> bool {
        self.session.hud.lock().has_tab_list_override()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> TextComponent {
        TextComponent::plain(value.to_owned())
    }

    fn bar(progress: f32) -> BossBar {
        BossBar {
            title: text("Mine"),
            progress,
            color: BossBarColor::Yellow,
            overlay: BossBarOverlay::Progress,
            flags: 0,
        }
    }

    #[test]
    fn sidebar_sends_only_changed_lines_and_resets_removed_ones() {
        let mut hud = PlayerHud::default();
        let first = hud.set_sidebar(text("Prison"), &[text("a"), text("b"), text("c")]);
        // add objective, display slot, three scores
        assert_eq!(first.len(), 5);
        let second = hud.set_sidebar(text("Prison"), &[text("a"), text("B")]);
        assert!(matches!(
            second.as_slice(),
            [HudPacket::Score(score), HudPacket::Reset(reset)]
                if score.owner == "#l1" && reset.owner == "#l2"
        ));
        assert!(
            hud.set_sidebar(text("Prison"), &[text("a"), text("B")])
                .is_empty()
        );
        assert_eq!(hud.clear_sidebar().len(), 1);
        assert!(hud.clear_sidebar().is_empty());
    }

    #[test]
    fn boss_bar_updates_send_one_operation_per_changed_field() {
        let mut hud = PlayerHud::default();
        let id = Uuid::from_u128(7);
        assert!(matches!(
            hud.set_boss_bar(id, bar(2.0)).as_slice(),
            [HudPacket::Boss(CBossEvent { operation: BossEventOperation::Add { progress, .. }, .. })]
                if (*progress - 1.0).abs() < f32::EPSILON
        ));
        assert!(hud.set_boss_bar(id, bar(1.0)).is_empty());
        assert!(matches!(
            hud.set_boss_bar(id, bar(0.5)).as_slice(),
            [HudPacket::Boss(CBossEvent {
                operation: BossEventOperation::UpdateProgress(_),
                ..
            })]
        ));
        assert_eq!(hud.remove_boss_bar(id).len(), 1);
        assert!(hud.remove_boss_bar(id).is_empty());
    }
}

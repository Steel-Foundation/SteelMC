//! Player helpers for sending client-side title displays.

use steel_protocol::packets::game::{
    CClearTitles, CSetActionBarText, CSetSubtitleText, CSetTitleText, CSetTitlesAnimation,
};
use text_components::TextComponent;

use crate::player::Player;

impl Player {
    /// Sends title text to the player.
    pub fn send_title(&self, text: impl Into<TextComponent>) {
        self.send_packet(CSetTitleText::new(text));
    }

    /// Sends subtitle text to the player.
    pub fn send_subtitle(&self, text: impl Into<TextComponent>) {
        self.send_packet(CSetSubtitleText::new(text));
    }

    /// Sends action-bar text to the player.
    pub fn send_action_bar(&self, text: impl Into<TextComponent>) {
        self.send_packet(CSetActionBarText::new(text));
    }

    /// Sets title animation durations in client ticks.
    pub fn send_title_times(&self, fade_in: i32, stay: i32, fade_out: i32) {
        self.send_packet(CSetTitlesAnimation::new(fade_in, stay, fade_out));
    }

    /// Clears the player's current title display while preserving its timings.
    pub fn clear_titles(&self) {
        self.send_packet(CClearTitles::new(false));
    }

    /// Clears the player's current title display and resets its timings.
    pub fn reset_titles(&self) {
        self.send_packet(CClearTitles::new(true));
    }
}

//! Per-player HUD: private sidebar, boss bars and tab list header/footer.
//!
//! Each player has one sidebar owned by whichever extension last set it; lines are text
//! (JSON components or plain), top line first, at most 15. Updates only send changed lines,
//! so calling [`Server::set_sidebar`](crate::Server::set_sidebar) every tick is cheap.
//! Boss bars are identified by a extension-chosen [`Uuid`](crate::players::Uuid). A tab list
//! override replaces the server's TPS header/footer for that player until cleared.

use stabby::string::String as ApiString;

/// Vanilla boss bar color.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarColor {
    /// Pink.
    Pink,
    /// Blue.
    Blue,
    /// Red.
    Red,
    /// Green.
    Green,
    /// Yellow.
    Yellow,
    /// Purple.
    Purple,
    /// White.
    White,
}

/// Vanilla boss bar segmentation.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarStyle {
    /// Continuous bar.
    Progress,
    /// Six segments.
    Notched6,
    /// Ten segments.
    Notched10,
    /// Twelve segments.
    Notched12,
    /// Twenty segments.
    Notched20,
}

/// A boss bar as shown to one player.
#[stabby::stabby]
#[derive(Debug, Clone, PartialEq)]
pub struct BossBar {
    /// Title (JSON text or plain).
    pub title: ApiString,
    /// Fill from 0 to 1.
    pub progress: f32,
    /// Color.
    pub color: BarColor,
    /// Segmentation.
    pub style: BarStyle,
    /// Darkens the sky.
    pub darken_screen: bool,
    /// Plays boss music.
    pub boss_music: bool,
    /// Adds fog.
    pub fog: bool,
}

impl BossBar {
    /// A plain bar with `title`, `progress` and `color`.
    #[must_use]
    pub fn new(title: &str, progress: f32, color: BarColor) -> Self {
        Self {
            title: title.into(),
            progress,
            color,
            style: BarStyle::Progress,
            darken_screen: false,
            boss_music: false,
            fog: false,
        }
    }

    /// Sets the segmentation.
    #[must_use]
    pub const fn style(mut self, style: BarStyle) -> Self {
        self.style = style;
        self
    }
}

#[stabby::host_api]
/// HUD bindings on `Server`.
pub mod api {
    use super::BossBar;
    use crate::{ApiResult, PlayerId, Server, players::Uuid};
    use stabby::{str::Str, string::String as ApiString, vec::Vec as ApiVec};

    #[cfg(feature = "host")]
    use crate::host::hud as host;

    impl<'a> Server<'a> {
        /// Shows or updates a player's sidebar (top line first, at most 15 lines).
        pub fn set_sidebar(
            &self,
            player: PlayerId,
            title: Str<'_>,
            lines: ApiVec<ApiString>,
        ) -> ApiResult<()> {
            host::set_sidebar(self.host(), player, title.as_str(), &lines)
                .map_err(ApiString::from)
                .into()
        }

        /// Removes a player's sidebar.
        pub fn clear_sidebar(&self, player: PlayerId) -> ApiResult<()> {
            host::clear_sidebar(self.host(), player)
                .map_err(ApiString::from)
                .into()
        }

        /// Shows or updates a boss bar; only changed fields are sent.
        pub fn set_boss_bar(&self, player: PlayerId, id: Uuid, bar: BossBar) -> ApiResult<()> {
            host::set_boss_bar(self.host(), player, id, &bar)
                .map_err(ApiString::from)
                .into()
        }

        /// Removes a boss bar.
        pub fn remove_boss_bar(&self, player: PlayerId, id: Uuid) -> ApiResult<()> {
            host::remove_boss_bar(self.host(), player, id)
                .map_err(ApiString::from)
                .into()
        }

        /// Replaces a player's tab list header and footer, hiding the server's TPS footer.
        pub fn set_tab_list(
            &self,
            player: PlayerId,
            header: Str<'_>,
            footer: Str<'_>,
        ) -> ApiResult<()> {
            host::set_tab_list(self.host(), player, header.as_str(), footer.as_str())
                .map_err(ApiString::from)
                .into()
        }

        /// Returns a player's tab list to the server default.
        pub fn clear_tab_list(&self, player: PlayerId) -> ApiResult<()> {
            host::clear_tab_list(self.host(), player)
                .map_err(ApiString::from)
                .into()
        }
    }
}

//! Player, permission, inventory and messaging access through [`Server`](crate::Server).
//!
//! Players are addressed by [`PlayerId`](crate::PlayerId); every call re-checks that the same
//! connection is still online, so stale identities return errors instead of acting on another
//! session. Text parameters accept vanilla JSON components or plain text (see
//! [`text`](crate::text)).

use stabby::string::String as ApiString;

/// A persistent profile UUID.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Uuid {
    /// Big-endian UUID bytes.
    pub bytes: [u8; 16],
}

impl Uuid {
    /// Parses a hyphenated or plain hexadecimal UUID.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let digits: Vec<u8> = text.bytes().filter(|byte| *byte != b'-').collect();
        if digits.len() != 32 {
            return None;
        }
        let mut bytes = [0; 16];
        for (index, pair) in digits.chunks(2).enumerate() {
            let pair = core::str::from_utf8(pair).ok()?;
            bytes[index] = u8::from_str_radix(pair, 16).ok()?;
        }
        Some(Self { bytes })
    }
}

impl core::fmt::Display for Uuid {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (index, byte) in self.bytes.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                formatter.write_str("-")?;
            }
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl crate::PlayerId {
    /// Persistent profile UUID.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        Uuid { bytes: self.bytes }
    }
}

/// Tri-state permission resolution from Steel's permission groups.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionValue {
    /// Explicitly granted.
    Allow,
    /// Explicitly denied.
    Deny,
    /// Not set; the caller applies its own default.
    Unset,
}

impl PermissionValue {
    /// Resolves `Unset` to `default`.
    #[must_use]
    pub const fn or(self, default: bool) -> bool {
        match self {
            Self::Allow => true,
            Self::Deny => false,
            Self::Unset => default,
        }
    }
}

/// The stored type of a [`MetadataValue`].
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataKind {
    /// Boolean metadata.
    Bool,
    /// Integer metadata.
    Integer,
    /// Text metadata.
    String,
}

/// A permission metadata value such as a chat prefix or a numeric limit.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct MetadataValue {
    /// Text form of the value, for every kind.
    pub text: ApiString,
    /// Integer value when `kind` is `Integer`.
    pub integer: i64,
    /// Stored type.
    pub kind: MetadataKind,
    /// Boolean value when `kind` is `Bool`.
    pub boolean: bool,
}

#[stabby::host_api]
/// Player bindings on `Server`.
pub mod api {
    use super::{MetadataValue, PermissionValue, Uuid};
    use crate::events::Location;
    use crate::{ApiResult, GameMode, PlayerId, Server};
    use stabby::{
        option::Option as StableOption, str::Str, string::String as ApiString, vec::Vec as ApiVec,
    };

    #[cfg(feature = "host")]
    use crate::host::players as host;

    impl<'a> Server<'a> {
        /// Finds an online player by name, ignoring case.
        #[must_use]
        pub fn player_by_name(&self, name: Str<'_>) -> StableOption<PlayerId> {
            host::by_name(self.host(), name.as_str()).into()
        }

        /// Finds an online player by profile UUID.
        #[must_use]
        pub fn player_by_uuid(&self, uuid: Uuid) -> StableOption<PlayerId> {
            host::by_uuid(self.host(), uuid.bytes).into()
        }

        /// Profile name.
        #[must_use]
        pub fn player_name(&self, player: PlayerId) -> ApiResult<ApiString> {
            host::with_player(self.host(), player, |p| {
                ApiString::from(p.gameprofile.name())
            })
            .map_err(ApiString::from)
            .into()
        }

        /// Current world, position and rotation.
        #[must_use]
        pub fn player_location(&self, player: PlayerId) -> ApiResult<Location> {
            host::with_player(self.host(), player, |p| {
                crate::host::events::player_location(p)
            })
            .map_err(ApiString::from)
            .into()
        }

        /// Teleports to a loaded world in the player's domain, firing the teleport event.
        pub fn teleport_to(&self, player: PlayerId, destination: Location) -> ApiResult<()> {
            host::teleport_to(self.host(), player, &destination)
                .map_err(ApiString::from)
                .into()
        }

        /// Keys of every loaded world.
        #[must_use]
        pub fn world_keys(&self) -> ApiVec<ApiString> {
            host::world_keys(self.host())
        }

        /// Current health.
        #[must_use]
        pub fn health(&self, player: PlayerId) -> ApiResult<f32> {
            host::health(self.host(), player)
        }

        /// Maximum health.
        #[must_use]
        pub fn max_health(&self, player: PlayerId) -> ApiResult<f32> {
            host::max_health(self.host(), player)
        }

        /// Sets health, clamped to `0..=max_health`.
        pub fn set_health(&self, player: PlayerId, health: f32) -> ApiResult<()> {
            host::set_health(self.host(), player, health)
                .map_err(ApiString::from)
                .into()
        }

        /// Current food level (0–20).
        #[must_use]
        pub fn food_level(&self, player: PlayerId) -> ApiResult<i32> {
            host::with_player(self.host(), player, |p| p.food_data.lock().food_level)
                .map_err(ApiString::from)
                .into()
        }

        /// Sets food level (clamped to 0–20) and saturation (clamped to the food level).
        pub fn set_food(&self, player: PlayerId, food: i32, saturation: f32) -> ApiResult<()> {
            host::set_food(self.host(), player, food, saturation)
                .map_err(ApiString::from)
                .into()
        }

        /// Current game mode.
        #[must_use]
        pub fn game_mode(&self, player: PlayerId) -> ApiResult<GameMode> {
            host::game_mode(self.host(), player)
        }

        /// Whether the player may fly outside creative and spectator.
        #[must_use]
        pub fn allow_flight(&self, player: PlayerId) -> ApiResult<bool> {
            host::with_player(self.host(), player, |p| p.abilities.lock().may_fly)
                .map_err(ApiString::from)
                .into()
        }

        /// Grants or revokes flight; revoking also stops flying.
        pub fn set_allow_flight(&self, player: PlayerId, allow: bool) -> ApiResult<()> {
            host::set_allow_flight(self.host(), player, allow)
                .map_err(ApiString::from)
                .into()
        }

        /// Disconnects the player with a reason.
        pub fn kick(&self, player: PlayerId, reason: Str<'_>) -> ApiResult<()> {
            host::kick(self.host(), player, reason.as_str())
                .map_err(ApiString::from)
                .into()
        }

        /// Sends a chat message.
        pub fn send_message(&self, player: PlayerId, text: Str<'_>) -> ApiResult<()> {
            host::send_message(self.host(), player, text.as_str())
                .map_err(ApiString::from)
                .into()
        }

        /// Shows text above the hotbar.
        pub fn send_action_bar(&self, player: PlayerId, text: Str<'_>) -> ApiResult<()> {
            host::send_action_bar(self.host(), player, text.as_str())
                .map_err(ApiString::from)
                .into()
        }

        /// Shows a title and subtitle with fade timings in ticks.
        pub fn send_title(
            &self,
            player: PlayerId,
            title: Str<'_>,
            subtitle: Str<'_>,
            fade_in: i32,
            stay: i32,
            fade_out: i32,
        ) -> ApiResult<()> {
            host::send_title(
                self.host(),
                player,
                title.as_str(),
                subtitle.as_str(),
                (fade_in, stay, fade_out),
            )
            .map_err(ApiString::from)
            .into()
        }

        /// Sends a chat message to every online player and the console.
        pub fn broadcast(&self, text: Str<'_>) {
            host::broadcast(self.host(), text.as_str());
        }

        /// Resolves a dotted permission node against the player's groups and overrides.
        #[must_use]
        pub fn permission(&self, player: PlayerId, node: Str<'_>) -> ApiResult<PermissionValue> {
            host::permission(self.host(), player, node.as_str())
                .map_err(ApiString::from)
                .into()
        }

        /// Whether the player holds `node`, using `default` when it is unset.
        #[must_use]
        pub fn has_permission(&self, player: PlayerId, node: Str<'_>, default: bool) -> bool {
            host::permission(self.host(), player, node.as_str())
                .is_ok_and(|value| value.or(default))
        }

        /// Resolves a metadata key such as `vault:prefix` for the player.
        pub fn permission_metadata(
            &self,
            player: PlayerId,
            key: Str<'_>,
        ) -> ApiResult<StableOption<MetadataValue>> {
            host::permission_metadata(self.host(), player, key.as_str())
                .map(Into::into)
                .map_err(ApiString::from)
                .into()
        }

        /// The player's permission groups, highest priority first.
        #[must_use]
        pub fn permission_groups(&self, player: PlayerId) -> ApiResult<ApiVec<ApiString>> {
            host::with_player(self.host(), player, |p| {
                p.permission_groups()
                    .into_iter()
                    .map(ApiString::from)
                    .collect::<ApiVec<_>>()
            })
            .map_err(ApiString::from)
            .into()
        }

        /// Every configured permission group.
        #[must_use]
        pub fn group_names(&self) -> ApiVec<ApiString> {
            self.host()
                .permission_groups
                .group_names()
                .into_iter()
                .map(Into::into)
                .collect()
        }

        /// Gives `count` items of `item`, optionally named with text (empty for none).
        /// Items that do not fit are dropped at the player's feet; returns how many were dropped.
        pub fn give_item(
            &self,
            player: PlayerId,
            item: Str<'_>,
            count: i32,
            name: Str<'_>,
        ) -> ApiResult<i32> {
            host::give_item(self.host(), player, item.as_str(), count, name.as_str())
                .map_err(ApiString::from)
                .into()
        }

        /// Key of the item in the main hand (`minecraft:air` when empty).
        #[must_use]
        pub fn held_item(&self, player: PlayerId) -> ApiResult<ApiString> {
            host::with_player(self.host(), player, |p| {
                ApiString::from(
                    p.inventory
                        .lock()
                        .get_selected_item()
                        .item()
                        .key
                        .to_string(),
                )
            })
            .map_err(ApiString::from)
            .into()
        }

        /// Current server tick.
        #[must_use]
        pub fn current_tick(&self) -> u64 {
            self.host().current_tick()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Uuid;

    #[test]
    fn uuid_round_trips_through_text() {
        let text = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
        let uuid = Uuid::parse(text);
        assert_eq!(uuid.map(|uuid| uuid.to_string()).as_deref(), Some(text));
        assert_eq!(Uuid::parse("069a79f444e94726a5befca90e38aaf5"), uuid);
        assert!(Uuid::parse("not-a-uuid").is_none());
    }
}

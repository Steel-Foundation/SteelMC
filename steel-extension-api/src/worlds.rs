//! World access outside callbacks, time and weather, item drops, entities and extension
//! messaging.
//!
//! [`Server::with_world`] borrows a loaded world for the duration of a closure, so scheduled
//! tasks and commands can act on worlds other than the one their callback received.

use crate::{Server, World};
use core::ptr;

/// Vanilla weather state.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weather {
    /// Clear sky.
    Clear,
    /// Rain or snow.
    Rain,
    /// Thunderstorm.
    Thunder,
}

#[stabby::host_api]
/// World, entity and messaging bindings.
pub mod api {
    use super::Weather;
    use crate::{ApiResult, PlayerId, Position, Server, World, items::ItemSpec};
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;
    use stabby::{str::Str, vec::Vec as ApiVec};

    #[cfg(feature = "host")]
    use crate::host::worlds as host;

    impl<'a> Server<'a> {
        /// Calls `callback(world, token)` with a loaded world (`domain:world` key).
        /// Prefer [`Server::with_world`], which wraps a closure.
        pub fn visit_world(
            &self,
            key: Str<'_>,
            callback: extern "C" fn(World<'_>, u64),
            token: u64,
        ) -> ApiResult<()> {
            host::visit_world(self.host(), key.as_str(), callback, token)
                .map_err(ApiString::from)
                .into()
        }

        /// Sends a custom payload on `channel` (such as `myextension:sync`) to a player.
        pub fn send_plugin_message(
            &self,
            player: PlayerId,
            channel: Str<'_>,
            data: ApiVec<u8>,
        ) -> ApiResult<()> {
            host::send_plugin_message(self.host(), player, channel.as_str(), &data)
                .map_err(ApiString::from)
                .into()
        }
    }

    impl<'a> World<'a> {
        /// Vanilla game time: ticks this world has run, unaffected by `/time set`.
        #[must_use]
        pub fn game_time(&self) -> i64 {
            self.host().game_time()
        }

        /// Total ticks of the dimension's default clock (day time); 0 without one.
        #[must_use]
        pub fn day_time(&self) -> i64 {
            host::day_time(self.host())
        }

        /// Sets the default clock's total ticks; errors when the dimension has none.
        pub fn set_day_time(&self, ticks: i64) -> ApiResult<()> {
            host::set_day_time(self.host(), ticks)
                .map_err(ApiString::from)
                .into()
        }

        /// Current weather.
        #[must_use]
        pub fn weather(&self) -> Weather {
            host::weather(self.host())
        }

        /// Sets the weather for `duration` ticks, like `/weather`.
        pub fn set_weather(&self, weather: Weather, duration: i32) -> ApiResult<()> {
            host::set_weather(self.host(), weather, duration)
                .map_err(ApiString::from)
                .into()
        }

        /// Spawns an item entity; returns its entity id.
        pub fn drop_item(&self, position: Position, item: ItemSpec) -> ApiResult<i32> {
            host::drop_item(self.host(), position, &item)
                .map_err(ApiString::from)
                .into()
        }

        /// Removes a non-player entity; returns whether it existed.
        pub fn remove_entity(&self, entity_id: i32) -> ApiResult<bool> {
            host::remove_entity(self.host(), entity_id)
                .map_err(ApiString::from)
                .into()
        }

        /// Shows a harmless lightning strike (flash, bolt and thunder) to nearby players;
        /// returns how many saw it.
        #[must_use]
        pub fn strike_lightning_effect(&self, position: Position) -> i32 {
            host::strike_lightning_effect(self.host(), position)
        }
    }
}

extern "C" fn visit_trampoline(world: World<'_>, token: u64) {
    // SAFETY: `token` is the address of the `&mut dyn FnMut(World<'_>)` that `with_world`
    // keeps alive on its stack for the whole synchronous `visit_world` call.
    let callback = unsafe { &mut *(token as usize as *mut &mut dyn FnMut(World<'_>)) };
    callback(world);
}

impl Server<'_> {
    /// Runs `f` with a loaded world (`domain:world` key) and returns its result.
    ///
    /// # Errors
    ///
    /// Returns the host diagnostic when the world is not loaded.
    pub fn with_world<R>(&self, key: &str, f: impl FnOnce(World<'_>) -> R) -> Result<R, String> {
        let mut f = Some(f);
        let mut result = None;
        let mut call = |world: World<'_>| {
            if let Some(f) = f.take() {
                result = Some(f(world));
            }
        };
        let mut callback: &mut dyn FnMut(World<'_>) = &mut call;
        let token = ptr::from_mut(&mut callback) as usize as u64;
        Result::from(self.visit_world(key.into(), visit_trampoline, token))
            .map_err(|error| error.to_string())?;
        result.ok_or_else(|| "world callback did not run".to_owned())
    }
}

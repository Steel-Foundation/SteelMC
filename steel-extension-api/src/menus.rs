//! Chest GUIs.
//!
//! [`Server::open_menu`](crate::Server::open_menu) shows a read-only chest of 1–6 rows filled
//! with [`ItemSpec`](crate::items::ItemSpec) icons. Items never move: every click is
//! cancelled and delivered to the menu's [`MenuHandler`] as a [`MenuClick`]. Slots
//! `0..rows*9` are the chest; higher slots are the player's inventory below it. Handlers may
//! give items, charge money, update icons with
//! [`Server::set_menu_item`](crate::Server::set_menu_item), open another menu or close it.
//!
//! Register a handler once (for example during init) and reuse it for every menu it serves:
//!
//! ```ignore
//! let handler = menus::handler(|server, click| { /* ... */ });
//! server.open_menu_with(player, "Enchants", 3, icons, handler)?;
//! ```

use crate::{PlayerId, Server, callbacks::Table, events::ClickKind, items::ItemSpec};

/// One click in a extension menu.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuClick {
    /// Menu ID returned when it was opened.
    pub menu: u64,
    /// Clicking player.
    pub player: PlayerId,
    /// Clicked slot; -999 outside the window.
    pub slot: i32,
    /// Mouse button or hotbar key.
    pub button: i32,
    /// Click kind.
    pub kind: ClickKind,
}

#[stabby::host_api]
/// Menu bindings on `Server`.
pub mod api {
    use super::MenuClick;
    use crate::{ApiResult, PlayerId, Server, items::ItemSpec};
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;
    use stabby::{str::Str, vec::Vec as ApiVec};

    #[cfg(feature = "host")]
    use crate::host::menus as host;

    impl<'a> Server<'a> {
        /// Opens a read-only chest GUI; `callback(server, click, token)` receives every click.
        /// Returns the menu ID. Prefer [`Server::open_menu_with`].
        pub fn open_menu(
            &self,
            player: PlayerId,
            title: Str<'_>,
            rows: i32,
            items: ApiVec<ItemSpec>,
            callback: extern "C" fn(Server<'_>, MenuClick, u64),
            token: u64,
        ) -> ApiResult<u64> {
            host::open_menu(
                self.host(),
                player,
                title.as_str(),
                rows,
                &items,
                callback,
                token,
            )
            .map_err(ApiString::from)
            .into()
        }

        /// Replaces one icon in the player's open extension menu.
        pub fn set_menu_item(&self, player: PlayerId, slot: i32, item: ItemSpec) -> ApiResult<()> {
            host::set_menu_item(self.host(), player, slot, &item)
                .map_err(ApiString::from)
                .into()
        }

        /// Closes the player's extension menu, if one is open.
        pub fn close_menu(&self, player: PlayerId) -> ApiResult<()> {
            host::close_menu(self.host(), player)
                .map_err(ApiString::from)
                .into()
        }
    }
}

type ClickFn = dyn Fn(&Server<'_>, MenuClick) + Send + Sync;

static HANDLERS: Table<ClickFn> = Table::new();

extern "C" fn dispatch(server: Server<'_>, click: MenuClick, token: u64) {
    if let Some(handler) = HANDLERS.get(token) {
        handler(&server, click);
    }
}

/// A registered click handler, reusable for any number of menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuHandler {
    token: u64,
}

/// Registers a click handler for the extension's lifetime.
pub fn handler(f: impl Fn(&Server<'_>, MenuClick) + Send + Sync + 'static) -> MenuHandler {
    MenuHandler {
        token: HANDLERS.insert(Box::new(f)),
    }
}

impl Server<'_> {
    /// Opens a read-only chest GUI handled by `handler`; returns the menu ID.
    ///
    /// # Errors
    ///
    /// Returns the host diagnostic for invalid rows, items or an offline player.
    pub fn open_menu_with(
        &self,
        player: PlayerId,
        title: &str,
        rows: i32,
        items: impl IntoIterator<Item = ItemSpec>,
        handler: MenuHandler,
    ) -> Result<u64, String> {
        Result::from(self.open_menu(
            player,
            title.into(),
            rows,
            items.into_iter().collect(),
            dispatch,
            handler.token,
        ))
        .map_err(|error| error.to_string())
    }
}

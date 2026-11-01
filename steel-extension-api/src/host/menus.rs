//! Host registry of open extension menus and the click router.
//!
//! A extension menu is a read-only chest ([`display_chest`]) whose clicks are routed from the
//! inventory click event: the router runs first, cancels the click (so nothing moves and the
//! client is resynchronized) and calls the extension's handler. The event fires before any
//! container lock is taken, so handlers may change inventories or open another menu.
use super::{events::click_kind, events::player_id, items::to_stack};
use crate::{PlayerId, Server, items::ItemSpec, menus::MenuClick};
use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use steel_core::{
    entity::Entity as _,
    extension::{
        EventHandler, EventPriority, ExtensionHooks,
        events::{InventoryClickEvent, InventoryCloseEvent, PlayerQuitEvent},
    },
    inventory::{
        container::{Container, SimpleContainer},
        menu::kinds::display_chest,
    },
    player::Player,
    server::Server as CoreServer,
};
use steel_registry::item_stack::ItemStack;
use steel_utils::locks::{IntoShared, Shared, SyncMutex};

/// Extension click callback.
pub(crate) type MenuCallback = extern "C" fn(Server<'_>, MenuClick, u64);

/// (player UUID bytes, entity ID, container ID).
type MenuKey = ([u8; 16], i32, i32);

#[derive(Clone)]
struct OpenMenu {
    id: u64,
    callback: MenuCallback,
    token: u64,
    container: Shared<SimpleContainer>,
}

static OPEN_MENUS: SyncMutex<BTreeMap<MenuKey, OpenMenu>> = SyncMutex::new(BTreeMap::new());
static NEXT_MENU: AtomicU64 = AtomicU64::new(1);

fn key(player: &Player, container_id: i32) -> MenuKey {
    (
        *player.gameprofile.id().as_bytes(),
        player.id(),
        container_id,
    )
}

fn entries_of(player: &Player) -> RangeInclusive<MenuKey> {
    let uuid = *player.gameprofile.id().as_bytes();
    (uuid, player.id(), i32::MIN)..=(uuid, player.id(), i32::MAX)
}

/// The extension menu shown in `container_id` for this player, if any.
pub(crate) fn menu_of(player: &Player, container_id: i32) -> Option<u64> {
    OPEN_MENUS
        .lock()
        .get(&key(player, container_id))
        .map(|menu| menu.id)
}

fn forget_all(player: &Player) {
    OPEN_MENUS
        .lock()
        .retain(|key, _| !entries_of(player).contains(key));
}

pub(crate) fn open_menu(
    server: &CoreServer,
    id: PlayerId,
    title: &str,
    rows: i32,
    items: &[ItemSpec],
    callback: MenuCallback,
    token: u64,
) -> Result<u64, String> {
    let rows = usize::try_from(rows)
        .ok()
        .filter(|rows| (1..=6).contains(rows))
        .ok_or("menus have 1 to 6 rows")?;
    if items.len() > rows * 9 {
        return Err(format!("{} items do not fit in {rows} rows", items.len()));
    }
    let mut stacks = items.iter().map(to_stack).collect::<Result<Vec<_>, _>>()?;
    stacks.resize_with(rows * 9, ItemStack::empty);
    let container = SimpleContainer::from_items(stacks).into_shared();
    let player = super::players::find(server, id)?;
    let menu_id = NEXT_MENU.fetch_add(1, Ordering::Relaxed);
    let factory_player = Arc::clone(&player);
    player.open_menu(super::events::parse_text(title), move |context| {
        let container_id = i32::from(context.container_id);
        OPEN_MENUS.lock().insert(
            key(&factory_player, container_id),
            OpenMenu {
                id: menu_id,
                callback,
                token,
                container: Arc::clone(&container),
            },
        );
        display_chest(
            &factory_player.inventory,
            context.container_id,
            container,
            rows,
        )
    });
    Ok(menu_id)
}

pub(crate) fn set_menu_item(
    server: &CoreServer,
    id: PlayerId,
    slot: i32,
    item: &ItemSpec,
) -> Result<(), String> {
    let player = super::players::find(server, id)?;
    let container = OPEN_MENUS
        .lock()
        .range(entries_of(&player))
        .next()
        .map(|(_, menu)| Arc::clone(&menu.container))
        .ok_or("the player has no extension menu open")?;
    let stack = to_stack(item)?;
    let mut container = container.lock();
    let index = usize::try_from(slot)
        .ok()
        .filter(|index| *index < container.get_container_size())
        .ok_or_else(|| format!("menu slot {slot} is out of range"))?;
    container.set_item(index, stack);
    container.set_changed();
    Ok(())
}

pub(crate) fn close_menu(server: &CoreServer, id: PlayerId) -> Result<(), String> {
    let player = super::players::find(server, id)?;
    if OPEN_MENUS
        .lock()
        .range(entries_of(&player))
        .next()
        .is_none()
    {
        return Ok(());
    }
    player.close_container();
    Ok(())
}

struct ClickRouter;

impl EventHandler<InventoryClickEvent> for ClickRouter {
    fn handle(&self, event: &mut InventoryClickEvent, _priority: EventPriority) {
        let menu = OPEN_MENUS
            .lock()
            .get(&key(&event.player, event.container_id))
            .cloned();
        let Some(menu) = menu else {
            return;
        };
        event.cancelled = true;
        let click = MenuClick {
            player: player_id(&event.player),
            menu: menu.id,
            slot: i32::from(event.slot),
            button: i32::from(event.button),
            kind: click_kind(event.click),
        };
        let server = event.player.server();
        (menu.callback)(Server::from_host(&server), click, menu.token);
    }
}

struct CloseRouter;

impl EventHandler<InventoryCloseEvent> for CloseRouter {
    fn handle(&self, event: &mut InventoryCloseEvent, _priority: EventPriority) {
        OPEN_MENUS
            .lock()
            .remove(&key(&event.player, i32::from(event.container_id)));
    }
}

struct QuitRouter;

impl EventHandler<PlayerQuitEvent> for QuitRouter {
    fn handle(&self, event: &mut PlayerQuitEvent, _priority: EventPriority) {
        forget_all(&event.player);
    }
}

/// Installs the menu routers ahead of every extension handler.
pub(crate) fn install(hooks: &mut ExtensionHooks) {
    hooks
        .events
        .inventory_click
        .subscribe(EventPriority::Lowest, false, Box::new(ClickRouter));
    hooks
        .events
        .inventory_close
        .subscribe(EventPriority::Monitor, false, Box::new(CloseRouter));
    hooks
        .events
        .quit
        .subscribe(EventPriority::Monitor, false, Box::new(QuitRouter));
}

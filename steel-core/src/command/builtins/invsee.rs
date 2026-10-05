use std::{
    array,
    ops::Range,
    sync::{Arc, Weak},
};

use steel_registry::vanilla_menu_types;
use steel_utils::Identifier;
use text_components::TextComponent;

use super::super::{
    brigadier::{CommandNodeBuilder, CommandSyntaxError},
    execution::{
        CommandPermissionSource, CommandSource, SteelArgumentType, SteelCommandRuntime, argument,
        literal,
    },
    registration::{CommandRegistration, CommandRegistrationError},
};
use crate::entity::Entity;
use crate::inventory::menu::Menu;
use crate::inventory::prelude::*;
use crate::inventory::slots::CraftingHandler;
use crate::permission::{PermissionExpr, PermissionKey, PermissionKeyError};
use crate::player::player_inventory::{PlayerInventory, armor_equipment};
use crate::player::{Player, connection::NetworkConnection};

const INVSEE_PERMISSION: &str = "steel.command.invsee";
const MODIFY_PERMISSION: &str = "steel.command.invsee.modify";

pub(super) fn registration() -> Result<CommandRegistration<CommandSource>, CommandRegistrationError>
{
    let id = Identifier::from_steel("invsee");
    let (access_permission, modify_permission) = invsee_permissions().map_err(|source| {
        CommandRegistrationError::InvalidExplicitPermission {
            id: id.clone(),
            source,
        }
    })?;
    let command_modify = modify_permission.clone();
    Ok(
        CommandRegistration::new(id, move |_| command(command_modify))
            .permission(access_permission),
    )
}

fn invsee_permissions() -> Result<(PermissionExpr, PermissionExpr), PermissionKeyError> {
    let access = PermissionExpr::key(PermissionKey::parse(INVSEE_PERMISSION)?);
    let modify = PermissionExpr::key(PermissionKey::parse(MODIFY_PERMISSION)?);
    Ok((PermissionExpr::Any(vec![access, modify.clone()]), modify))
}

fn command(
    modify_permission: PermissionExpr,
) -> CommandNodeBuilder<CommandSource, SteelCommandRuntime> {
    literal("invsee").then(
        argument("target", SteelArgumentType::player()).executes(move |ctx| {
            let target = ctx.player("target")?;
            let Some(source) = ctx.source().player() else {
                return Err(CommandSyntaxError::dynamic(TextComponent::const_plain(
                    "you cannot use this command from the console",
                )));
            };
            ensure_same_domain(source, &target)?;
            // Command permissions belong to the initiating authorization even
            // when `/execute as` changes which player receives the menu. Capture
            // the resulting mode once when the menu opens.
            let modify = ctx.source().has_permission(&modify_permission);
            let opener = Arc::clone(source);
            let menu_source = Arc::clone(source);
            opener.open_menu(target.display_name(), move |context| {
                invsee(context.container_id, &menu_source, &target, modify)
            });
            Ok(1)
        }),
    )
}

fn ensure_same_domain(source: &Player, target: &Player) -> Result<(), CommandSyntaxError> {
    if source.is_domain_switching() || target.is_domain_switching() {
        return Err(CommandSyntaxError::dynamic(
            "Invsee is unavailable while a player is switching domains",
        ));
    }
    let source_world = source.get_world();
    let target_world = target.get_world();
    if source_world.domain() == target_world.domain() {
        return Ok(());
    }
    Err(CommandSyntaxError::dynamic(
        "Invsee cannot open inventories across Steel domains",
    ))
}

fn invsee(container_id: u8, source: &Arc<Player>, target: &Arc<Player>, modify: bool) -> Menu {
    let mut b = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X5, container_id);

    let kind = if modify {
        SectionKind::Normal
    } else {
        SectionKind::Display
    };

    let target_inventory = b.player_inventory_with(&target.inventory, &kind);

    let armor_kind = if modify {
        SectionKind::restricted(|index, item| item.is_equippable_in_slot(armor_equipment(index)))
    } else {
        SectionKind::Display
    };
    let armor = b.section_at(
        &target.inventory,
        PlayerInventory::ARMOR_TOP_DOWN,
        armor_kind,
    );
    let offhand = b.section_at(&target.inventory, [PlayerInventory::SLOT_OFFHAND], kind);

    let crafting_handler = target.inventory_crafting_handler();
    let crafting_container = crafting_handler.crafting_container();
    let crafting = if modify {
        b.section_all_with(crafting_container, SectionKind::take_only())
    } else {
        b.section_all_with(crafting_container, SectionKind::Display)
    };
    b.register_container(crafting_handler.result_container());

    let target_slots = 0..b.slot_count();
    let viewer = b.player_inventory(&source.inventory);

    if modify {
        let inventories_alias = Arc::ptr_eq(&source.inventory, &target.inventory);
        if !inventories_alias {
            b.route(
                target_inventory.all(),
                viewer.all(),
                FillDirection::Backward,
            );
            b.route(
                viewer.all(),
                [target_inventory.all(), armor, offhand],
                FillDirection::Forward,
            );
        }
        b.route(
            [armor, offhand, crafting],
            viewer.all(),
            FillDirection::Backward,
        );
    }

    b.build(InvseeMenuKind {
        target: Arc::downgrade(target),
        target_inventory_id: ContainerId::from_arc(&target.inventory),
        domain: target.get_world().domain().into(),
        modify,
        target_slots,
        crafting,
        crafting_handler,
        inventory_before_click: None,
    })
}

struct InvseeMenuKind {
    target: Weak<Player>,
    target_inventory_id: ContainerId,
    domain: Box<str>,
    modify: bool,
    target_slots: Range<usize>,
    crafting: Section,
    crafting_handler: CraftingHandler,
    inventory_before_click: Option<[ItemStack; PlayerInventory::SLOT_OFFHAND + 1]>,
}

// SAFETY: This Steel-owned key uniquely identifies the concrete menu kind
// within the process.
unsafe impl steel_utils::DowncastType for InvseeMenuKind {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:menu/invsee");
}

impl MenuKind for InvseeMenuKind {
    fn on_drag(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _action: QuickCraft,
        _player: &Player,
    ) -> ClickOutcome {
        self.snapshot_inventory_before_click(guard);
        ClickOutcome::Fallthrough
    }

    fn on_open(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.crafting_handler.update_result(guard);
    }

    fn slots_changed(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        _player: &Player,
    ) {
        self.crafting_handler.update_result(guard);
        self.queue_changed_target_inventory(guard);
    }

    fn on_slot_clicked(
        &mut self,
        _behavior: &mut MenuBehavior,
        guard: &mut ContainerLockGuard,
        click: Click,
        _player: &Player,
    ) -> ClickOutcome {
        let Some(slot) = click.slot() else {
            return ClickOutcome::Fallthrough;
        };
        self.snapshot_inventory_before_click(guard);
        if (!self.modify && self.target_slots.contains(&slot))
            || (self.crafting.contains(slot) && matches!(click, Click::Clone { .. }))
        {
            ClickOutcome::Consume
        } else {
            ClickOutcome::Fallthrough
        }
    }

    fn can_drag_to(&self, slot_index: usize) -> bool {
        if self.modify {
            !self.crafting.contains(slot_index)
        } else {
            !self.target_slots.contains(&slot_index)
        }
    }

    fn can_take_item_for_pick_all(&self, _carried: &ItemStack, slot_index: usize) -> bool {
        self.modify || !self.target_slots.contains(&slot_index)
    }

    fn still_valid(&self, _behavior: &MenuBehavior, player: &Player) -> bool {
        let Some(target) = self.target.upgrade() else {
            return false;
        };
        let player_world = player.get_world();
        let target_world = target.get_world();
        !player.is_domain_switching()
            && !target.connection.closed()
            && !target.is_domain_switching()
            && player_world.domain() == self.domain.as_ref()
            && target_world.domain() == self.domain.as_ref()
    }
}

impl InvseeMenuKind {
    fn snapshot_inventory_before_click(&mut self, guard: &ContainerLockGuard) {
        if !self.modify {
            return;
        }
        let Some(inventory) = guard.get(self.target_inventory_id) else {
            unreachable!("invsee always locks the target inventory");
        };
        self.inventory_before_click = Some(array::from_fn(|slot| inventory.get_item(slot).clone()));
    }

    fn queue_changed_target_inventory(&mut self, guard: &ContainerLockGuard) {
        let Some(previous) = self.inventory_before_click.take() else {
            return;
        };
        let Some(inventory) = guard.get(self.target_inventory_id) else {
            unreachable!("invsee always locks the target inventory");
        };
        let changed_slots = previous
            .iter()
            .enumerate()
            .filter_map(|(slot, previous)| {
                let current = inventory.get_item(slot);
                (!ItemStack::matches(previous, current)).then_some(slot)
            })
            .collect::<Vec<_>>();
        if changed_slots.is_empty() {
            return;
        }
        let Some(target) = self.target.upgrade() else {
            return;
        };
        target.request_inventory_resync(changed_slots);
    }
}

#[cfg(test)]
mod tests;

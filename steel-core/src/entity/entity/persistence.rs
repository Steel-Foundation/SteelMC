//! Saving to NBT, spawn data and dropping items.

use super::*;
use steel_utils::nbt::NbtValueInput as _;

/// Saving to NBT, spawn data and dropping items.
///
/// Implemented for every [`Entity`]; entity types change behavior through
/// [`EntityHooks`], never by implementing this trait.
pub trait EntityPersistence: Entity {
    /// Returns the vanilla-shaped entity NBT used by command predicates.
    ///
    /// Mirrors `NbtPredicate.getEntityTagToCompare`: base and type-specific
    /// `Entity.saveWithoutId` data, passengers, and a player's selected item.
    fn nbt_for_data_compare(&self) -> NbtCompound {
        let mut nbt = NbtCompound::new();
        let position = self.vehicle().map_or_else(
            || self.position(),
            |vehicle| {
                DVec3::new(
                    vehicle.position().x,
                    self.position().y,
                    vehicle.position().z,
                )
            },
        );
        let velocity = self.velocity();
        let (yaw, pitch) = self.rotation();
        let fire_freeze = self.fire_freeze_state();

        nbt.insert(
            "Pos",
            NbtList::Double(vec![position.x, position.y, position.z]),
        );
        nbt.insert(
            "Motion",
            NbtList::Double(vec![velocity.x, velocity.y, velocity.z]),
        );
        nbt.insert("Rotation", NbtList::Float(vec![yaw, pitch]));
        nbt.insert("fall_distance", self.fall_distance());
        nbt.insert(
            "Fire",
            NbtTag::Short(fire_freeze.remaining_fire_ticks() as i16),
        );
        nbt.insert("Air", NbtTag::Short(self.air_supply() as i16));
        nbt.insert("OnGround", nbt_bool(self.on_ground()));
        nbt.insert("Invulnerable", nbt_bool(self.is_invulnerable()));
        nbt.insert("PortalCooldown", self.portal_cooldown());
        nbt.insert(
            "UUID",
            NbtTag::IntArray(self.uuid().to_int_array().to_vec()),
        );

        if let Some(custom_name) = self.custom_name() {
            nbt.insert("CustomName", custom_name.to_codec_nbt());
        }
        if self.is_custom_name_visible() {
            nbt.insert("CustomNameVisible", nbt_bool(true));
        }
        if self.is_silent() {
            nbt.insert("Silent", nbt_bool(true));
        }
        if self.is_no_gravity() {
            nbt.insert("NoGravity", nbt_bool(true));
        }
        if self.has_glowing_tag() {
            nbt.insert("Glowing", nbt_bool(true));
        }
        if fire_freeze.ticks_frozen() > 0 {
            nbt.insert("TicksFrozen", fire_freeze.ticks_frozen());
        }
        if fire_freeze.has_visual_fire() {
            nbt.insert("HasVisualFire", nbt_bool(true));
        }

        let tags = self.tags();
        if !tags.is_empty() {
            nbt.insert("Tags", NbtList::from(tags));
        }
        let custom_data = self.custom_data();
        if !custom_data.is_empty() {
            nbt.insert("data", NbtTag::Compound(custom_data));
        }

        if let Some(living) = self.as_living_entity() {
            living.save_command_nbt(&mut nbt);
        }
        self.save_additional(&mut nbt);

        if let Some(player) = self.as_player() {
            player.save_command_nbt(&mut nbt);
        }

        let passengers = self
            .passengers()
            .into_iter()
            .filter_map(|passenger| passenger.nbt_for_passenger_save())
            .collect::<Vec<_>>();
        if !passengers.is_empty() {
            nbt.insert("Passengers", NbtList::Compound(passengers));
        }

        if let Some(player) = self.as_player() {
            let inventory = player.inventory.lock();
            let selected_item = inventory.get_selected_item();
            if !selected_item.is_empty() {
                nbt.insert("SelectedItem", selected_item.to_nbt_tag_ref());
            }
        }

        nbt
    }

    /// Returns passenger-save NBT including the entity type id.
    fn nbt_for_passenger_save(&self) -> Option<NbtCompound> {
        if !self.removal_reason().is_none_or(RemovalReason::should_save)
            || !self.entity_type().can_serialize
        {
            return None;
        }

        let mut nbt = self.nbt_for_data_compare();
        nbt.insert("id", self.entity_type().key.to_string());
        Some(nbt)
    }

    /// Spawns an item at this entity's location.
    ///
    /// The item spawns at the entity's position with the given Y offset and
    /// has a default pickup delay.
    ///
    /// Returns `None` if the item stack is empty or the entity has no world.
    fn spawn_at_location(
        &self,
        item: ItemStack,
        y_offset: f64,
    ) -> Option<Arc<entities::ItemEntity>> {
        let world = self.level()?;
        let pos = self.position();
        let entity = world.spawn_item(DVec3::new(pos.x, pos.y + y_offset, pos.z), item)?;
        entity.set_default_pickup_delay();
        Some(entity)
    }

    /// Spawns an item at this entity's location plus a vanilla attachment offset.
    fn spawn_at_location_with_offset(
        &self,
        item: ItemStack,
        offset: DVec3,
    ) -> Option<Arc<entities::ItemEntity>> {
        let world = self.level()?;
        let entity = world.spawn_item(self.position() + offset, item)?;
        entity.set_default_pickup_delay();
        Some(entity)
    }

    /// Default for [`Entity::apply_implicit_item_components`].
    fn apply_implicit_item_components_default(&self, _item_stack: &ItemStack) {}

    /// Spawn-item data is merged into the entity's current save state before it
    /// is loaded. Keeping that merge at the entity boundary preserves defaults
    /// and entity-specific state when the component only overrides one field.
    fn apply_spawn_data(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        if let Some(position) = read_nbt_dvec3(&nbt, "Pos")
            && position.is_finite()
        {
            self.base()
                .set_position_local(super::clamp_loaded_entity_position(position));
        }

        if let Some(motion) = read_nbt_dvec3(&nbt, "Motion") {
            self.set_velocity(sanitize_nbt_motion(motion));
        }

        if let Some((yaw, pitch)) = read_nbt_rotation(&nbt, "Rotation") {
            self.set_rotation((yaw, pitch));
            self.set_y_head_rot(yaw);
            self.set_y_body_rot(yaw);
        }

        if let Some(fall_distance) = nbt
            .double_value("fall_distance")
            .or_else(|| nbt.double_value("FallDistance"))
        {
            self.set_fall_distance(fall_distance);
        }

        if let Some(on_ground) = nbt.byte_value("OnGround") {
            self.set_on_ground(on_ground != 0);
        }

        let mut save_data = self.base().save_data();
        if let Some(air_supply) = nbt.int_value("Air") {
            save_data.air_supply = air_supply;
        }
        if let Some(portal_cooldown) = nbt.int_value("PortalCooldown") {
            save_data.portal_cooldown = portal_cooldown;
        }
        if let Some(no_gravity) = nbt.byte_value("NoGravity") {
            save_data.no_gravity = no_gravity != 0;
        }
        if let Some(invulnerable) = nbt.byte_value("Invulnerable") {
            save_data.invulnerable = invulnerable != 0;
        }
        if let Some(custom_name) = nbt
            .get("CustomName")
            .and_then(|tag| TextComponent::from_nbt(&tag.to_owned()))
        {
            save_data.custom_name = Some(custom_name);
        }
        if let Some(custom_name_visible) = nbt.byte_value("CustomNameVisible") {
            save_data.custom_name_visible = custom_name_visible != 0;
        }
        if let Some(silent) = nbt.byte_value("Silent") {
            save_data.silent = silent != 0;
        }
        if let Some(glowing) = nbt.byte_value("Glowing") {
            save_data.glowing = glowing != 0;
        }
        if let Some(tags) = nbt.list("Tags").and_then(|list| list.strings()) {
            save_data.tags = tags
                .iter()
                .take(MAX_ENTITY_TAGS)
                .map(|tag| tag.to_str().into_owned())
                .collect::<BTreeSet<_>>();
        }
        if let Some(custom_data) = nbt.compound("data") {
            save_data.custom_data = custom_data.to_owned();
        }
        self.base().replace_save_data(save_data);

        if let Some(remaining_fire_ticks) = nbt.short_value("Fire") {
            self.set_remaining_fire_ticks(i32::from(remaining_fire_ticks));
        }
        if let Some(ticks_frozen) = nbt.int_value("TicksFrozen") {
            self.set_ticks_frozen(ticks_frozen);
        }
        if let Some(has_visual_fire) = nbt.byte_value("HasVisualFire") {
            self.base().set_visual_fire(has_visual_fire != 0);
        }

        self.load_additional(nbt);
        self.set_old_position_to_current();
        self.base().set_old_rotation_to_current();
        self.sync_base_entity_data();
    }
}

impl<T: Entity + ?Sized> EntityPersistence for T {}

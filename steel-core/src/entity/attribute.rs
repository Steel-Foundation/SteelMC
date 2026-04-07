//! Runtime entity attribute system.
//!
use core::iter;

pub use steel_protocol::packets::game::AttributeModifierOperation;
use steel_protocol::packets::game::{AttributeModifierData, AttributeSnapshot};
use steel_registry::attribute::AttributeRef;
use steel_registry::entity_types::EntityTypeRef;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt};
use steel_utils::Identifier;

/// Growable bitmask for tracking dirty attribute IDs.
pub struct DirtySet {
    chunks: Vec<u64>,
}

impl Default for DirtySet {
    fn default() -> Self {
        Self::new()
    }
}

impl DirtySet {
    /// Creates an empty dirty set
    #[must_use]
    pub const fn new() -> Self {
        Self { chunks: Vec::new() }
    }

    /// Marks an attribute ID as dirty
    pub fn mark(&mut self, id: u16) {
        let chunk = id as usize / 64;
        let bit = id as usize % 64;
        if chunk >= self.chunks.len() {
            self.chunks.resize(chunk + 1, 0);
        }
        self.chunks[chunk] |= 1 << bit;
    }

    /// Returns `true` if no attributes are dirty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.iter().all(|&c| c == 0)
    }

    /// Drains all marked IDs, clearing the set
    pub fn drain(&mut self) -> impl Iterator<Item = u16> + '_ {
        self.chunks.iter_mut().enumerate().flat_map(|(i, chunk)| {
            let mut bits = *chunk;
            *chunk = 0;
            iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let bit = bits.trailing_zeros() as u16;
                bits &= bits - 1;
                Some(i as u16 * 64 + bit)
            })
        })
    }
}

/// A modifier applied to an attribute instance
#[derive(Clone, Debug)]
pub struct AttributeModifier {
    /// Unique identifier (e.g. `minecraft:sprinting`)
    pub id: Identifier,
    /// The modifier value
    pub amount: f64,
    /// How the modifier is applied during calculation
    pub operation: AttributeModifierOperation,
}

/// Runtime state for a single attribute on an entity
pub struct AttributeInstance {
    attribute: AttributeRef,
    base_value: f64,
    modifiers: Vec<AttributeModifier>,
    /// Parallel to `modifiers` `true` means the modifier survives serialization
    persistent: Vec<bool>,
    cached_value: f64,
}

impl AttributeInstance {
    fn new(attribute: AttributeRef, base_value: f64) -> Self {
        let cached = attribute.sanitize_value(base_value);
        Self {
            attribute,
            base_value,
            modifiers: Vec::new(),
            persistent: Vec::new(),
            cached_value: cached,
        }
    }

    /// Returns the attribute definition
    #[must_use]
    pub const fn attribute(&self) -> AttributeRef {
        self.attribute
    }

    /// Returns the base value before modifiers
    #[must_use]
    pub const fn base_value(&self) -> f64 {
        self.base_value
    }

    /// Sets the base value and recalculates. Returns `true` if changed
    #[expect(
        clippy::float_cmp,
        reason = "vanilla uses exact base value equality for dirty checks"
    )]
    pub fn set_base_value(&mut self, value: f64) -> bool {
        if self.base_value == value {
            return false;
        }
        self.base_value = value;
        self.recalculate();
        true
    }

    /// Returns the final calculated value (base + modifiers, clamped)
    #[must_use]
    pub const fn value(&self) -> f64 {
        self.cached_value
    }

    /// Adds a modifier. Returns `false` if a modifier with this ID already exists
    pub fn add_modifier(&mut self, modifier: AttributeModifier, persistent: bool) -> bool {
        if self.modifiers.iter().any(|m| m.id == modifier.id) {
            return false;
        }
        self.modifiers.push(modifier);
        self.persistent.push(persistent);
        self.recalculate();
        true
    }

    /// Adds or replaces a modifier. Returns `true` if the value actually changed.
    #[expect(
        clippy::float_cmp,
        reason = "exact equality is intentional — we want to skip recalculation when the modifier is identical"
    )]
    pub fn set_modifier(&mut self, modifier: AttributeModifier, persistent: bool) -> bool {
        if let Some(idx) = self.modifiers.iter().position(|m| m.id == modifier.id) {
            let existing = &self.modifiers[idx];
            if existing.amount == modifier.amount
                && existing.operation == modifier.operation
                && self.persistent[idx] == persistent
            {
                return false;
            }
            self.modifiers[idx] = modifier;
            self.persistent[idx] = persistent;
        } else {
            self.modifiers.push(modifier);
            self.persistent.push(persistent);
        }
        self.recalculate();
        true
    }

    /// Removes a modifier by ID, Returns `true` if it existed
    pub fn remove_modifier(&mut self, id: &Identifier) -> bool {
        let Some(idx) = self.modifiers.iter().position(|m| m.id == *id) else {
            return false;
        };
        self.modifiers.swap_remove(idx);
        self.persistent.swap_remove(idx);
        self.recalculate();
        true
    }

    /// Returns an iterator over permanent modifiers (for serialization)
    pub fn permanent_modifiers(&self) -> impl Iterator<Item = &AttributeModifier> {
        self.modifiers
            .iter()
            .zip(self.persistent.iter())
            .filter(|&(_, p)| *p)
            .map(|(m, _)| m)
    }

    /// Removes all transient (non-persistent) modifiers. Returns `true` if any were removed
    fn remove_transient_modifiers(&mut self) -> bool {
        if !self.persistent.iter().any(|&p| !p) {
            return false;
        }
        let mut i = 0;
        while i < self.modifiers.len() {
            if self.persistent[i] {
                i += 1;
            } else {
                self.modifiers.swap_remove(i);
                self.persistent.swap_remove(i);
            }
        }
        self.recalculate();
        true
    }

    /// Three-phase vanilla calculation:
    /// 1. `ADD_VALUE`
    /// 2. `ADD_MULTIPLIED_BASE`
    /// 3. `ADD_MULTIPLIED_TOTAL`
    fn recalculate(&mut self) {
        let mut base = self.base_value;
        for m in &self.modifiers {
            if m.operation == AttributeModifierOperation::AddValue {
                base += m.amount;
            }
        }

        let mut result = base;
        for m in &self.modifiers {
            if m.operation == AttributeModifierOperation::AddMultipliedBase {
                result += base * m.amount;
            }
        }
        for m in &self.modifiers {
            if m.operation == AttributeModifierOperation::AddMultipliedTotal {
                result *= 1.0 + m.amount;
            }
        }

        self.cached_value = self.attribute.sanitize_value(result);
    }

    /// Builds a network snapshot for `CUpdateAttributes`
    fn to_snapshot(&self, attribute_id: i32) -> AttributeSnapshot {
        AttributeSnapshot {
            attribute_id,
            base_value: self.base_value,
            modifiers: self
                .modifiers
                .iter()
                .map(|m| AttributeModifierData {
                    id: m.id.clone(),
                    amount: m.amount,
                    operation: m.operation,
                })
                .collect(),
        }
    }
}

/// Per-entity container for attribute instances with dirty tracking
///
/// Indexed by attribute registry ID. Two dirty sets match vanilla's design:
/// - `to_update`: all dirty attributes, drained for server-side effects
/// - `to_sync`: syncable dirty attributes, drained for network packets
pub struct AttributeMap {
    instances: Vec<Option<AttributeInstance>>,
    to_update: DirtySet,
    to_sync: DirtySet,
}

impl AttributeMap {
    /// Creates an `AttributeMap` from an entity type's default attributes
    // TODO: Add AttributeSupplier for lazy instantiation when mob entities are implemented
    #[must_use]
    pub fn new_for_entity(entity_type: EntityTypeRef) -> Self {
        let attr_count = REGISTRY.attributes.len();
        let mut instances = Vec::with_capacity(attr_count);
        instances.resize_with(attr_count, || None);

        for &(attr_name, base_value) in entity_type.default_attributes {
            let key = Identifier::vanilla_static(attr_name);
            if let Some(id) = REGISTRY.attributes.id_from_key(&key)
                && let Some(attr) = REGISTRY.attributes.by_id(id)
            {
                instances[id] = Some(AttributeInstance::new(attr, base_value));
            }
        }

        Self {
            instances,
            to_update: DirtySet::new(),
            to_sync: DirtySet::new(),
        }
    }

    /// Returns `true` if the entity has this attribute registered
    #[must_use]
    pub fn has_attribute(&self, attribute: AttributeRef) -> bool {
        attribute
            .try_id()
            .and_then(|id| self.instances.get(id))
            .is_some_and(Option::is_some)
    }

    /// Gets the calculated value of an attribute
    #[must_use]
    pub fn get_value(&self, attribute: AttributeRef) -> Option<f64> {
        let id = attribute.try_id()?;
        self.instances
            .get(id)?
            .as_ref()
            .map(AttributeInstance::value)
    }

    /// Gets the base value of an attribute
    #[must_use]
    pub fn get_base_value(&self, attribute: AttributeRef) -> Option<f64> {
        let id = attribute.try_id()?;
        self.instances
            .get(id)?
            .as_ref()
            .map(AttributeInstance::base_value)
    }

    /// Gets a reference to an attribute instance
    #[must_use]
    pub fn get_instance(&self, attribute: AttributeRef) -> Option<&AttributeInstance> {
        let id = attribute.try_id()?;
        self.instances.get(id)?.as_ref()
    }

    /// Sets the base value of an attribute
    pub fn set_base_value(&mut self, attribute: AttributeRef, value: f64) {
        let Some(id) = attribute.try_id() else { return };
        let Some(Some(instance)) = self.instances.get_mut(id) else {
            return;
        };
        if instance.set_base_value(value) {
            self.mark_dirty(id, attribute);
        }
    }

    /// Adds a modifier to an attribute. Returns `false` if the modifier ID already exists
    pub fn add_modifier(
        &mut self,
        attribute: AttributeRef,
        modifier: AttributeModifier,
        persistent: bool,
    ) -> bool {
        let Some(id) = attribute.try_id() else {
            return false;
        };
        let Some(Some(instance)) = self.instances.get_mut(id) else {
            return false;
        };
        if instance.add_modifier(modifier, persistent) {
            self.mark_dirty(id, attribute);
            true
        } else {
            false
        }
    }

    /// Adds or replaces a modifier on an attribute
    pub fn set_modifier(
        &mut self,
        attribute: AttributeRef,
        modifier: AttributeModifier,
        persistent: bool,
    ) {
        let Some(id) = attribute.try_id() else { return };
        let Some(Some(instance)) = self.instances.get_mut(id) else {
            return;
        };
        if instance.set_modifier(modifier, persistent) {
            self.mark_dirty(id, attribute);
        }
    }

    /// Removes a modifier from an attribute. Returns `true` if it existed
    pub fn remove_modifier(&mut self, attribute: AttributeRef, modifier_id: &Identifier) -> bool {
        let Some(id) = attribute.try_id() else {
            return false;
        };
        let Some(Some(instance)) = self.instances.get_mut(id) else {
            return false;
        };
        if instance.remove_modifier(modifier_id) {
            self.mark_dirty(id, attribute);
            true
        } else {
            false
        }
    }

    fn mark_dirty(&mut self, id: usize, attribute: AttributeRef) {
        let id = id as u16;
        self.to_update.mark(id);
        if attribute.syncable {
            self.to_sync.mark(id);
        }
    }

    /// Returns `true` if there are dirty attributes needing server-side effects
    #[must_use]
    pub fn has_dirty_updates(&self) -> bool {
        !self.to_update.is_empty()
    }

    /// Returns `true` if there are syncable dirty attributes needing network send
    #[must_use]
    pub fn has_dirty_sync(&self) -> bool {
        !self.to_sync.is_empty()
    }

    /// Drains `to_update` and returns the dirty attribute refs
    pub fn drain_dirty_updates(&mut self) -> Vec<AttributeRef> {
        self.to_update
            .drain()
            .filter_map(|id| {
                self.instances
                    .get(id as usize)?
                    .as_ref()
                    .map(|inst| inst.attribute)
            })
            .collect()
    }

    /// Drains `to_sync` and builds `AttributeSnapshot`s for `CUpdateAttributes`
    pub fn drain_dirty_sync(&mut self) -> Vec<AttributeSnapshot> {
        let ids: Vec<u16> = self.to_sync.drain().collect();
        let mut snapshots = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(Some(instance)) = self.instances.get(id as usize) {
                snapshots.push(instance.to_snapshot(i32::from(id)));
            }
        }
        snapshots
    }

    /// Returns snapshots for ALL syncable attributes (initial tracking sync)
    #[must_use]
    pub fn syncable_snapshots(&self) -> Vec<AttributeSnapshot> {
        let mut snapshots = Vec::new();
        for (id, slot) in self.instances.iter().enumerate() {
            if let Some(inst) = slot
                && inst.attribute.syncable
            {
                snapshots.push(inst.to_snapshot(id as i32));
            }
        }
        snapshots
    }

    /// Removes all transient modifiers (e.g. on respawn)
    pub fn remove_all_transient(&mut self) {
        let Self {
            instances,
            to_update,
            to_sync,
        } = self;
        for (id, slot) in instances.iter_mut().enumerate() {
            if let Some(inst) = slot
                && inst.remove_transient_modifiers()
            {
                to_update.mark(id as u16);
                if inst.attribute.syncable {
                    to_sync.mark(id as u16);
                }
            }
        }
    }
}

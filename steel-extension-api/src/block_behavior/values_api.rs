#[stabby::host_api]
pub mod api {
    #[cfg(feature = "host")]
    use crate::block_behavior::bridge::Bridge;
    #[cfg(feature = "host")]
    use crate::block_behavior::core_types::BonemealAction as NativeBonemealAction;
    use crate::block_behavior::types::{
        BlockInteraction, DamageSourceRef, Entity, Explosion, ExplosionDropConsumer, Vector3,
    };
    use crate::block_behavior::types::{BonemealAction, Inventory, ItemStack, Player};
    use crate::{ApiResult, BlockState};
    use crate::{BlockPos, World};
    use stabby::string::String as ApiString;
    use stabby::{option::Option as StableOption, str::Str};
    #[cfg(feature = "host")]
    use steel_registry::{REGISTRY, RegistryExt, item_stack::ItemStack as NativeItemStack};
    #[cfg(feature = "host")]
    use steel_utils::{BlockStateId, Identifier};

    impl BlockState {
        /// Reads a named state property, including a custom block's facing.
        #[must_use]
        pub fn property(self, name: Str<'_>) -> StableOption<ApiString> {
            REGISTRY
                .blocks
                .by_state_id(BlockStateId(self.id))
                .and_then(|_| {
                    REGISTRY
                        .blocks
                        .get_property_str(BlockStateId(self.id), name.as_str())
                })
                .map(ApiString::from)
                .into()
        }
    }

    #[expect(
        clippy::elidable_lifetime_names,
        reason = "generated host bridges require explicit receiver lifetimes for borrowed return values"
    )]
    impl<'a> Explosion<'a> {
        /// Borrows the world executing this explosion.
        #[must_use]
        pub fn world<'borrow>(&'borrow self) -> World<'borrow> {
            World::from_host(self.host().world())
        }
        /// Borrows the damage context carried by the explosion.
        #[must_use]
        pub fn damage_source<'borrow>(&'borrow self) -> DamageSourceRef<'borrow> {
            DamageSourceRef::from_host(self.host().damage_source())
        }
        /// Returns the block destruction and loot policy.
        #[must_use]
        pub fn block_interaction(&self) -> BlockInteraction {
            BlockInteraction::from_host(self.host().block_interaction())
        }
        /// Borrows the entity credited for this explosion, when present.
        #[must_use]
        pub fn indirect_source_entity<'borrow>(&'borrow self) -> StableOption<Entity<'borrow>> {
            self.host()
                .indirect_source_entity()
                .map(Entity::from_host)
                .into()
        }
        /// Borrows the entity that directly caused this explosion, when present.
        #[must_use]
        pub fn direct_source_entity<'borrow>(&'borrow self) -> StableOption<Entity<'borrow>> {
            self.host()
                .direct_source_entity()
                .map(Entity::from_host)
                .into()
        }
        /// Returns the explosion radius.
        #[must_use]
        pub fn radius(&self) -> f32 {
            self.host().radius()
        }
        /// Returns the explosion center.
        #[must_use]
        pub fn center(&self) -> Vector3 {
            Vector3::from_host(self.host().center())
        }
        /// Returns whether block-like entities may be affected.
        #[must_use]
        pub fn should_affect_blocklike_entities(&self) -> bool {
            self.host().should_affect_blocklike_entities()
        }
    }
    impl<'a> ExplosionDropConsumer<'a> {
        /// Transfers one explosion drop to the host's loot collector.
        pub fn accept(&mut self, stack: ItemStack, pos: BlockPos) {
            self.host_mut()(
                stack.into_host(),
                steel_utils::BlockPos::new(pos.x, pos.y, pos.z),
            );
        }
    }

    impl ItemStack {
        /// Creates a registered item with its default components.
        /// Rejects air, unknown keys, and counts outside the item's stack limit.
        #[must_use]
        pub fn from_key(key: Str<'_>, count: i32) -> ApiResult<Self> {
            let key = match key.as_str().parse::<Identifier>() {
                Ok(key) => key,
                Err(error) => return Err(ApiString::from(error)).into(),
            };
            let Some(item) = REGISTRY.items.by_key(&key) else {
                return Err(ApiString::from("unknown item key")).into();
            };
            let stack = NativeItemStack::with_count(item, count);
            if stack.is_empty() || count > stack.max_stack_size() {
                return Err(ApiString::from(
                    "count must be within the item's stack limit",
                ))
                .into();
            }
            Ok(Self::from_host(stack)).into()
        }

        /// Creates one item using the host's block-to-item association.
        /// Blocks without an associated item produce an empty stack.
        #[must_use]
        pub fn from_block(state: BlockState) -> ApiResult<Self> {
            let Some(block) = REGISTRY.blocks.by_state_id(BlockStateId(state.id)) else {
                return Err(ApiString::from("invalid block state")).into();
            };
            Ok(Self::from_host(NativeItemStack::new(
                REGISTRY.items.by_block(block),
            )))
            .into()
        }
    }

    impl<'a> Inventory<'a> {
        /// Tests the interacting hand's item against a host registry key.
        #[must_use]
        pub fn held_item_is(&self, key: Str<'_>) -> bool {
            let Ok(key) = key.as_str().parse::<Identifier>() else {
                return false;
            };
            let Some(item) = REGISTRY.items.by_key(&key) else {
                return false;
            };
            self.host().with_item(|stack| stack.is(item))
        }

        /// Applies durability damage, honoring the caller's creative-mode status.
        pub fn damage_item(&mut self, amount: i32, infinite_materials: bool) -> ApiResult<()> {
            if amount < 0 {
                return Err(ApiString::from("item damage must be nonnegative")).into();
            }
            self.host_mut()
                .with_item(|item| item.hurt_and_break(amount, infinite_materials));
            Ok(()).into()
        }
    }

    impl<'a> Player<'a> {
        /// Sits on a loaded block. The offset is the player's entity origin
        /// relative to that block; existing riding rules and sneak-dismount apply.
        pub fn sit(&self, pos: BlockPos, offset: crate::Position) -> ApiResult<bool> {
            self.host()
                .get_world()
                .sit_player(
                    self.host(),
                    steel_utils::BlockPos::new(pos.x, pos.y, pos.z),
                    glam::DVec3::new(offset.x, offset.y, offset.z),
                )
                .map_err(ApiString::from)
                .into()
        }

        /// Returns the block containing the player's feet, including negative coordinates.
        #[must_use]
        pub fn block_position(&self) -> BlockPos {
            use crate::block_behavior::core_types::Entity as _;
            let pos = self.host().block_position();
            BlockPos {
                x: pos.x(),
                y: pos.y(),
                z: pos.z(),
            }
        }

        /// Returns whether creative-mode material and durability rules apply.
        #[must_use]
        pub fn has_infinite_materials(&self) -> bool {
            self.host().has_infinite_materials()
        }
    }

    impl BonemealAction {
        /// Uses bonemeal to grow this block directly.
        #[must_use]
        pub fn grower() -> Self {
            Self::from_host(NativeBonemealAction::Grower)
        }

        /// Uses bonemeal to spread growth to neighboring blocks.
        #[must_use]
        pub fn neighbor_spreader() -> Self {
            Self::from_host(NativeBonemealAction::NeighborSpreader)
        }
    }
}

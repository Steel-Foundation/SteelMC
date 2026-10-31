#[stabby::host_api]
pub mod api {
    use crate::block_behavior::{
        block_entity::DynBlockEntityBehavior,
        types::{
            BlockEntity, BlockEntityCreation, BlockEntityRef, BlockEntityTicker, BorrowedNbt, Nbt,
            NbtMut, WeakWorld,
        },
    };
    #[cfg(feature = "host")]
    use crate::block_behavior::{
        block_entity::host::{Adapter, Base},
        core_types,
    };
    use crate::{ApiResult, BlockPos, BlockState};
    #[cfg(feature = "host")]
    use simdnbt::borrow::NbtCompound as NbtCompoundView;
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;
    use stabby::{option::Option as StableOption, str::Str};
    #[cfg(feature = "host")]
    use std::sync::{Arc, Weak};
    #[cfg(feature = "host")]
    use steel_registry::{REGISTRY, RegistryExt, blocks::block_state_ext::BlockStateExt};
    #[cfg(feature = "host")]
    use steel_utils::Identifier;

    impl BlockEntity {
        /// Creates extension entity data for an existing host-registered entity type.
        #[must_use]
        pub fn new(
            key: Str<'_>,
            level: WeakWorld,
            pos: BlockPos,
            state: BlockState,
            implementation: DynBlockEntityBehavior,
        ) -> ApiResult<Self> {
            let result = (|| -> Result<Self, ApiString> {
                let key: Identifier = key.as_str().parse().map_err(ApiString::from)?;
                let kind = REGISTRY
                    .block_entity_types
                    .by_key(&key)
                    .ok_or_else(|| ApiString::from(format!("unknown block entity type {key}")))?;
                let state = core_types::BlockStateId(state.id);
                if !kind.is_valid(state.get_block()) {
                    return Err(ApiString::from(
                        "block entity type does not accept this block state",
                    ));
                }
                let base = core_types::BlockEntityBase::new(
                    kind,
                    level.into_host(),
                    core_types::BlockPos::new(pos.x, pos.y, pos.z),
                    state,
                );
                Ok(Self::from_host(Arc::new(Adapter {
                    base: Base::New(base),
                    implementation,
                })))
            })();
            result.into()
        }

        /// Retains another reference to this entity; both handles share its actual data.
        #[must_use]
        pub fn share(&self) -> Self {
            Self::from_host(Arc::clone(self.host()))
        }

        /// Borrows this entity's original implementation for explicit delegation.
        #[must_use]
        pub fn borrow(&self) -> BlockEntityRef<'_> {
            BlockEntityRef::from_host(self.host().as_ref())
        }

        /// Replaces dynamic callbacks while retaining this entity's common metadata.
        /// The consumed entity remains available through any previously shared handle.
        #[must_use]
        pub fn with_behavior(self, implementation: DynBlockEntityBehavior) -> Self {
            Self::from_host(Arc::new(Adapter {
                base: Base::Original(self.into_host()),
                implementation,
            }))
        }
    }

    impl WeakWorld {
        /// Retains the same weak world reference for a factory or original delegate.
        #[must_use]
        pub fn share(&self) -> Self {
            Self::from_host(Weak::clone(self.host()))
        }
    }

    impl BlockEntityCreation {
        /// Returns a successfully created entity to the host.
        #[must_use]
        pub fn created(entity: BlockEntity) -> Self {
            Self::from_host(core_types::BlockEntityCreation::Created(entity.into_host()))
        }

        /// Explicitly requests no block entity for this state.
        #[must_use]
        pub fn no_entity() -> Self {
            Self::from_host(core_types::BlockEntityCreation::NoEntity)
        }

        /// Takes the original factory's entity, if it created one.
        pub fn into_created(self) -> StableOption<BlockEntity> {
            self.into_host()
                .into_created()
                .map(BlockEntity::from_host)
                .into()
        }
    }

    impl BlockEntityTicker {
        /// Selects the entity's tick callback for this exact registered entity type.
        #[must_use]
        pub fn for_entity_tick(key: Str<'_>) -> ApiResult<Self> {
            let result = (|| -> Result<Self, ApiString> {
                let key: Identifier = key.as_str().parse().map_err(ApiString::from)?;
                let kind = REGISTRY
                    .block_entity_types
                    .by_key(&key)
                    .ok_or_else(|| ApiString::from(format!("unknown block entity type {key}")))?;
                Ok(Self::from_host(
                    core_types::BlockEntityTicker::for_entity_tick(kind),
                ))
            })();
            result.into()
        }
    }

    impl<'a> BorrowedNbt<'a> {
        /// Reads an optional integer from the native save document.
        #[must_use]
        pub fn int(&self, key: Str<'_>) -> StableOption<i32> {
            let document: NbtCompoundView<'_, '_> = (*self.host()).into();
            document.int(key.as_str()).into()
        }
    }

    impl<'a> NbtMut<'a> {
        /// Writes entity data into the host's native save document.
        pub fn put_int(&mut self, key: Str<'_>, value: i32) {
            Nbt::put_native_int(self.host_mut(), key.as_str(), value);
        }
    }

    impl Nbt {
        /// Creates an owned native NBT document for client synchronization.
        #[must_use]
        pub fn new() -> Self {
            Self::from_host(core_types::NbtCompound::new())
        }

        /// Writes an integer into this document.
        pub fn put_int(&mut self, key: Str<'_>, value: i32) {
            Self::put_native_int(self.host_mut(), key.as_str(), value);
        }

        fn put_native_int(document: &mut core_types::NbtCompound, key: &str, value: i32) {
            while document.remove(key).is_some() {}
            document.insert(key, value);
        }
    }
}

use crate::block_behavior::types::Nbt;
impl Default for Nbt {
    fn default() -> Self {
        Self::new()
    }
}

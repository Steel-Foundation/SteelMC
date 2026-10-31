//! Typed conversions shared by the complete behavior contract.
use super::types::BlockInteraction;
use super::{InteractionResult, MovementResult, core_types, types::Vector3};
use crate::{BlockPos, BlockState, World};
use stabby::abi::IDeterminantProvider;
use stabby::{IStable, option::Option as StableOption};
use std::sync::Arc;

pub(crate) trait Bridge {
    type Host;
    fn from_host(value: Self::Host) -> Self;
    fn into_host(self) -> Self::Host;
}

macro_rules! identity {
    ($($ty:ty),*) => {$(impl Bridge for $ty {
        type Host = Self;
        fn from_host(value: Self) -> Self { value }
        fn into_host(self) -> Self { self }
    })*};
}
identity!(bool, i32, u8, f32, f64);

impl Bridge for BlockInteraction {
    type Host = core_types::BlockInteraction;
    fn from_host(value: Self::Host) -> Self {
        match value {
            Self::Host::Keep => Self::Keep,
            Self::Host::Destroy => Self::Destroy,
            Self::Host::DestroyWithDecay => Self::DestroyWithDecay,
        }
    }
    fn into_host(self) -> Self::Host {
        match self {
            Self::Keep => Self::Host::Keep,
            Self::Destroy => Self::Host::Destroy,
            Self::DestroyWithDecay => Self::Host::DestroyWithDecay,
        }
    }
}

impl<T: Bridge + IStable + IDeterminantProvider<()>> Bridge for StableOption<T> {
    type Host = Option<T::Host>;
    fn from_host(value: Self::Host) -> Self {
        value.map(T::from_host).into()
    }
    fn into_host(self) -> Self::Host {
        self.match_owned(|value| Some(value.into_host()), || None)
    }
}
impl Bridge for BlockState {
    type Host = core_types::BlockStateId;
    fn from_host(value: Self::Host) -> Self {
        Self { id: value.0 }
    }
    fn into_host(self) -> Self::Host {
        core_types::BlockStateId(self.id)
    }
}
impl Bridge for BlockPos {
    type Host = core_types::BlockPos;
    fn from_host(value: Self::Host) -> Self {
        Self {
            x: value.x(),
            y: value.y(),
            z: value.z(),
        }
    }
    fn into_host(self) -> Self::Host {
        core_types::BlockPos::new(self.x, self.y, self.z)
    }
}
impl<'a> Bridge for World<'a> {
    type Host = &'a Arc<core_types::World>;
    fn from_host(value: Self::Host) -> Self {
        Self::from_host(value)
    }
    fn into_host(self) -> Self::Host {
        self.host()
    }
}
impl Bridge for InteractionResult {
    type Host = core_types::InteractionResult;
    fn from_host(value: Self::Host) -> Self {
        value.into()
    }
    fn into_host(self) -> Self::Host {
        self.into()
    }
}
impl Bridge for MovementResult {
    type Host = (core_types::DVec3, bool);
    fn from_host((velocity, bounced): Self::Host) -> Self {
        Self {
            velocity: Vector3::from_host(velocity),
            bounced,
        }
    }
    fn into_host(self) -> Self::Host {
        (self.velocity.into_host(), self.bounced)
    }
}

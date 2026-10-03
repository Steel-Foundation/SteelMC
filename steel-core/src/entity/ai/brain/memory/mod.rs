//! Brain memory

mod map;
mod module_type;
mod value;

#[cfg(test)]
mod tests;

pub use map::Memories;
pub use module_type::{
    MEMORY_MODULE_TYPES, MemoryModuleType, MemoryModuleTypeEntry, MemoryModuleTypeRef,
    MemoryModuleTypeRegistry, MemoryModuleTypeRegistryLock,
};
pub use value::{MemoryValue, RememberedEntities, RememberedEntity};

pub(super) use module_type::init_memory_module_types;

/// The state an entry condition requires a memory slot to be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryStatus {
    /// The slot must hold a value.
    ValuePresent,
    /// The slot must be registered and empty.
    ValueAbsent,
    /// The slot must be registered; its contents do not matter.
    Registered,
}

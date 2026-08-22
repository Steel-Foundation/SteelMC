//! Steel Mod API and C-ABI compatibility interfaces.

use std::ffi::c_void;

/// Opaque context passed to mod initialization functions.
///
/// In future iterations, this will expose server hooks, events, and registries.
#[repr(C)]
pub struct ModContext {
    /// Opaque pointer to the server or engine instance.
    pub server_ptr: *mut c_void,
}

impl ModContext {
    /// Creates a new `ModContext` wrapping a raw server pointer.
    #[must_use]
    pub const fn new(server_ptr: *mut c_void) -> Self {
        Self { server_ptr }
    }
}

/// Signature for mod initialization entrypoints.
///
/// Returns 0 on success, non-zero error code on failure.
pub type SteelModInitFn = unsafe extern "C" fn(ctx: *const ModContext) -> i32;

/// Signature for mod shutdown entrypoints.
pub type SteelModShutdownFn = unsafe extern "C" fn(ctx: *const ModContext);

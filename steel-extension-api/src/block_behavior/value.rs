//! Host-owned values whose implementation and allocator stay on the host.

// Each declaration fixes one host type. Extensions cannot construct a pointer,
// change its type, or free it with the extension's allocator.
macro_rules! host_value {
    ($storage:ident, $name:ident $(<$($lt:lifetime),+>)? = $host:ty $(; $($access:ident),+)? ) => {
        mod $storage {
            #[cfg(feature = "host")]
            use super::*;
            #[doc = concat!("Opaque host-owned `", stringify!($name), "` value.")]
            #[stabby::stabby]
            pub struct Value $(<$($lt),+>)? {
                pointer: *mut (),
                destroy: unsafe extern "C" fn(*mut ()),
                borrow: core::marker::PhantomData<fn($($(&$lt (),)+)?) -> ($($(&$lt (),)+)?)>,
            }
            #[cfg(feature = "host")]
            impl $(<$($lt),+>)? Value $(<$($lt),+>)? {
                pub(crate) fn from_host(value: $host) -> Self {
                    unsafe extern "C" fn destroy<T>(pointer: *mut ()) {
                        // SAFETY: only from_host constructs this type; its paired
                        // destructor receives exactly the original Box allocation.
                        drop(unsafe { Box::from_raw(pointer.cast::<T>()) });
                    }
                    Self { pointer: Box::into_raw(Box::new(value)).cast(), destroy: destroy::<$host>,
                        borrow: core::marker::PhantomData }
                }
                pub(crate) fn into_host(self) -> $host {
                    let this = core::mem::ManuallyDrop::new(self);
                    // SAFETY: this nominal handle always contains this host type.
                    // Consuming it transfers the allocation exactly once.
                    *unsafe { Box::from_raw(this.pointer.cast::<$host>()) }
                }
                $(host_value!(@accessors [$($access),+] $host);)?
            }
            #[cfg(feature = "host")]
            impl $(<$($lt),+>)? crate::block_behavior::bridge::Bridge for Value $(<$($lt),+>)? {
                type Host = $host;
                fn from_host(value: Self::Host) -> Self { Self::from_host(value) }
                fn into_host(self) -> Self::Host { self.into_host() }
            }
            impl $(<$($lt),+>)? Drop for Value $(<$($lt),+>)? {
                fn drop(&mut self) {
                    // SAFETY: the creator pairs the destructor with its allocation.
                    // into_host suppresses Drop when ownership is transferred back.
                    unsafe { (self.destroy)(self.pointer) }
                }
            }
        }
        pub use $storage::Value as $name;
    };
    (@accessors [$first:ident $(,$rest:ident)*] $host:ty) => {
        host_value!(@access $first $host);
        $(host_value!(@accessors [$rest] $host);)*
    };
    (@access read $host:ty) => {
        pub(crate) fn host(&self) -> &$host {
            // SAFETY: from_host owns the allocation until self is dropped.
            unsafe { &*self.pointer.cast::<$host>() }
        }
    };
    (@access write $host:ty) => {
        pub(crate) fn host_mut(&mut self) -> &mut $host {
            // SAFETY: the non-Copy handle owns this allocation exclusively.
            unsafe { &mut *self.pointer.cast::<$host>() }
        }
    };
    (@access reborrow $host:ty) => {
        pub(crate) fn host_mut(&mut self) -> &mut <$host as core::ops::Deref>::Target {
            // SAFETY: this non-Copy handle owns a live exclusive host reference.
            // Reborrow its referent only for the duration of &mut self.
            unsafe { &mut **self.pointer.cast::<$host>() }
        }
    };

}
pub(super) use host_value;

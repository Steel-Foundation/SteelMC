//! Private payload pointers retain their original borrow through synchronous dispatch.

use super::{Action, Filter};
use crate::ApiResult;
use core::{marker::PhantomData, mem::transmute, ptr};
use stabby::abi::report::TypeReport;
use stabby::{host::ErasedFunction, str::Str, string::String as ApiString};

/// Shared payload borrowed for one synchronous action dispatch.
#[stabby::stabby]
pub struct ActionValue<'a> {
    data: *const (),
    report: &'static TypeReport,
    borrow: PhantomData<fn(&'a ()) -> &'a ()>,
}

impl<'a> ActionValue<'a> {
    pub(super) fn new<A: Action>(value: &'a A) -> Self {
        Self {
            data: ptr::from_ref(value).cast(),
            report: A::REPORT,
            borrow: PhantomData,
        }
    }
    #[cfg(feature = "host")]
    pub(super) const fn report(&self) -> &'static TypeReport {
        self.report
    }
    #[cfg(feature = "host")]
    pub(super) fn reborrow(&self) -> ActionValue<'_> {
        ActionValue {
            data: self.data,
            report: self.report,
            borrow: PhantomData,
        }
    }
}

/// Exclusive payload borrowed for one synchronous filter chain.
///
/// ```compile_fail
/// fn retain(value: steel_extension_api::hooks::FilterValue<'_>)
///     -> steel_extension_api::hooks::FilterValue<'static> { value }
/// ```
#[stabby::stabby]
pub struct FilterValue<'a> {
    data: *mut (),
    report: &'static TypeReport,
    borrow: PhantomData<fn(&'a mut ()) -> &'a mut ()>,
}

impl<'a> FilterValue<'a> {
    pub(super) fn new<F: Filter>(value: &'a mut F) -> Self {
        Self {
            data: ptr::from_mut(value).cast(),
            report: F::REPORT,
            borrow: PhantomData,
        }
    }
    #[cfg(feature = "host")]
    pub(super) const fn report(&self) -> &'static TypeReport {
        self.report
    }
    #[cfg(any(feature = "host", test))]
    pub(super) fn reborrow(&mut self) -> FilterValue<'_> {
        FilterValue {
            data: self.data,
            report: self.report,
            borrow: PhantomData,
        }
    }
}

/// Typed action callback paired with its checked payload contract.
/// Fields stay private so safe code cannot mismatch a function, thunk, or report.
#[stabby::stabby]
#[derive(Clone, Copy)]
pub struct ActionCallback {
    pub(super) tag: Str<'static>,
    pub(super) report: &'static TypeReport,
    pointer: ErasedFunction,
    invoke: unsafe extern "C" fn(ErasedFunction, *const (), &'static TypeReport) -> ApiResult<()>,
}

impl ActionCallback {
    pub(super) fn new<A: Action>(callback: extern "C" fn(&A)) -> Self {
        unsafe extern "C" fn invoke<A: Action>(
            pointer: ErasedFunction,
            data: *const (),
            report: &'static TypeReport,
        ) -> ApiResult<()> {
            if !A::REPORT.is_compatible(report) {
                return Err(ApiString::from("incompatible action payload ABI")).into();
            }
            // SAFETY: the private constructor pairs this thunk with exactly fn(&A).
            let callback = unsafe { transmute::<ErasedFunction, extern "C" fn(&A)>(pointer) };
            // SAFETY: new borrowed an ABI-stable A, the report matches, and the
            // synchronous value retains that shared borrow until this call returns.
            callback(unsafe { &*data.cast::<A>() });
            Ok(()).into()
        }
        // SAFETY: the pointer is restored only by the paired generic thunk above.
        let pointer = unsafe { transmute::<extern "C" fn(&A), ErasedFunction>(callback) };
        Self {
            tag: Str::new(A::TAG),
            report: A::REPORT,
            pointer,
            invoke: invoke::<A>,
        }
    }

    #[cfg(any(feature = "host", test))]
    pub(super) fn call(self, value: ActionValue<'_>) -> ApiResult<()> {
        // SAFETY: ActionValue retains a shared borrow of the reported payload;
        // the constructor paired this thunk and function pointer.
        unsafe { (self.invoke)(self.pointer, value.data, value.report) }
    }
}

/// Typed filter callback. Exclusive payloads are reborrowed for each invocation.
#[stabby::stabby]
#[derive(Clone, Copy)]
pub struct FilterCallback {
    pub(super) tag: Str<'static>,
    pub(super) report: &'static TypeReport,
    pointer: ErasedFunction,
    invoke: unsafe extern "C" fn(ErasedFunction, *mut (), &'static TypeReport) -> ApiResult<()>,
}

impl FilterCallback {
    pub(super) fn new<F: Filter>(callback: extern "C" fn(&mut F)) -> Self {
        unsafe extern "C" fn invoke<F: Filter>(
            pointer: ErasedFunction,
            data: *mut (),
            report: &'static TypeReport,
        ) -> ApiResult<()> {
            if !F::REPORT.is_compatible(report) {
                return Err(ApiString::from("incompatible filter payload ABI")).into();
            }
            // SAFETY: the private constructor pairs this thunk with exactly fn(&mut F).
            let callback = unsafe { transmute::<ErasedFunction, extern "C" fn(&mut F)>(pointer) };
            // SAFETY: new borrowed an ABI-stable F exclusively, the report matches,
            // and each synchronous invocation owns a distinct exclusive reborrow.
            callback(unsafe { &mut *data.cast::<F>() });
            Ok(()).into()
        }
        // SAFETY: only the paired generic thunk above restores this function pointer.
        let pointer = unsafe { transmute::<extern "C" fn(&mut F), ErasedFunction>(callback) };
        Self {
            tag: Str::new(F::TAG),
            report: F::REPORT,
            pointer,
            invoke: invoke::<F>,
        }
    }

    #[cfg(any(feature = "host", test))]
    pub(super) fn call(self, value: FilterValue<'_>) -> ApiResult<()> {
        // SAFETY: FilterValue owns the exclusive reborrow of the reported payload;
        // the constructor paired this thunk and function pointer.
        unsafe { (self.invoke)(self.pointer, value.data, value.report) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[stabby::stabby]
    struct First {
        count: u32,
    }
    #[stabby::stabby]
    struct Different {
        count: u64,
    }
    impl Action for First {
        const TAG: &'static str = "test:payload";
    }
    impl Action for Different {
        const TAG: &'static str = "test:payload";
    }
    impl Filter for First {
        const TAG: &'static str = "test:payload";
    }
    impl Filter for Different {
        const TAG: &'static str = "test:payload";
    }
    extern "C" fn action(value: &First) {
        assert_eq!(value.count, 7);
    }
    extern "C" fn filter(value: &mut First) {
        value.count += 1;
    }

    #[test]
    fn action_thunks_accept_matching_payloads_and_reject_incompatible_ones() {
        let callback = ActionCallback::new(action);
        assert!(
            callback
                .call(ActionValue::new(&First { count: 7 }))
                .match_owned(|()| true, |_| false)
        );
        assert!(
            callback
                .call(ActionValue::new(&Different { count: 7 }))
                .match_owned(|()| false, |_| true)
        );
    }
    #[test]
    fn filter_thunks_preserve_exclusive_mutation_and_reject_incompatible_payloads() {
        let callback = FilterCallback::new(filter);
        let mut value = First { count: 0 };
        let mut borrow = FilterValue::new(&mut value);
        for _ in 0..2 {
            assert!(
                callback
                    .call(borrow.reborrow())
                    .match_owned(|()| true, |_| false)
            );
        }
        assert_eq!(value.count, 2);
        let mut different = Different { count: 7 };
        assert!(
            callback
                .call(FilterValue::new(&mut different))
                .match_owned(|()| false, |_| true)
        );
        assert_eq!(different.count, 7);
    }
}

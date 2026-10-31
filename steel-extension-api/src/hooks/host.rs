use super::{ActionCallback, ActionValue, FilterCallback, FilterValue, HandlerOrdering, ordering};
use crate::ApiResult;
use stabby::abi::report::TypeReport;
use stabby::string::String as ApiString;
use std::{collections::BTreeMap, sync::Arc};
use steel_utils::locks::SyncRwLock;

struct Handler<C> {
    owner: String,
    callback: C,
    orderings: Vec<HandlerOrdering>,
}
struct Entry<C> {
    report: &'static TypeReport,
    handlers: Vec<Handler<C>>,
    order: Vec<usize>,
}
impl<C> Entry<C> {
    fn freeze(&mut self) -> Result<(), String> {
        let handlers: Vec<_> = self
            .handlers
            .iter()
            .map(|h| (h.owner.as_str(), h.orderings.as_slice()))
            .collect();
        self.order = ordering::resolve(&handlers)?;
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct EventRegistry {
    actions: BTreeMap<String, Entry<ActionCallback>>,
    filters: BTreeMap<String, Entry<FilterCallback>>,
}

impl EventRegistry {
    fn register<C>(
        table: &mut BTreeMap<String, Entry<C>>,
        owner: &str,
        tag: &str,
        report: &'static TypeReport,
        callback: C,
        orderings: &[HandlerOrdering],
    ) -> Result<(), String> {
        if !tag.contains(':') || tag.split(':').any(str::is_empty) {
            return Err("hook tags must have a non-empty owner namespace and path".into());
        }
        if orderings
            .iter()
            .any(|ordering| ordering.extension.as_str().is_empty())
        {
            return Err("handler ordering requires an extension name".into());
        }
        let entry = table.entry(tag.to_owned()).or_insert_with(|| Entry {
            report,
            handlers: Vec::new(),
            order: Vec::new(),
        });
        if !entry.report.is_compatible(report) {
            return Err(format!("incompatible payload ABI for hook {tag}"));
        }
        entry.handlers.push(Handler {
            owner: owner.to_owned(),
            callback,
            orderings: orderings.to_vec(),
        });
        Ok(())
    }
    fn freeze(&mut self) -> Result<(), String> {
        for (tag, entry) in &mut self.actions {
            entry.freeze().map_err(|e| format!("action {tag}: {e}"))?;
        }
        for (tag, entry) in &mut self.filters {
            entry.freeze().map_err(|e| format!("filter {tag}: {e}"))?;
        }
        Ok(())
    }
    fn do_action(&self, tag: &str, value: ActionValue<'_>) -> Result<(), ApiString> {
        let Some(entry) = self.actions.get(tag) else {
            return Ok(());
        };
        if !entry.report.is_compatible(value.report()) {
            return Err(format!("incompatible action payload ABI for {tag}").into());
        }
        for &index in &entry.order {
            // Each handler receives a shared borrow, never ownership of the payload.
            entry.handlers[index]
                .callback
                .call(ActionValue::reborrow(&value))
                .match_owned(Ok, Err)?;
        }
        Ok(())
    }
    fn apply_filters(&self, tag: &str, mut value: FilterValue<'_>) -> Result<(), ApiString> {
        let Some(entry) = self.filters.get(tag) else {
            return Ok(());
        };
        if !entry.report.is_compatible(value.report()) {
            return Err(format!("incompatible filter payload ABI for {tag}").into());
        }
        for &index in &entry.order {
            entry.handlers[index]
                .callback
                .call(value.reborrow())
                .match_owned(Ok, Err)?;
        }
        Ok(())
    }
}

struct State {
    pending: Option<EventRegistry>,
    frozen: Option<Arc<EventRegistry>>,
}
static EVENTS: SyncRwLock<State> = SyncRwLock::new(State {
    pending: Some(EventRegistry {
        actions: BTreeMap::new(),
        filters: BTreeMap::new(),
    }),
    frozen: None,
});

fn register(operation: impl FnOnce(&mut EventRegistry) -> Result<(), String>) -> ApiResult<()> {
    let result = (|| {
        let mut state = EVENTS.write();
        let pending = state
            .pending
            .as_mut()
            .ok_or("extension hook registration has already frozen")?;
        operation(pending)
    })();
    result.map_err(ApiString::from).into()
}

pub(crate) fn register_action(
    owner: &str,
    callback: ActionCallback,
    orderings: &[HandlerOrdering],
) -> ApiResult<()> {
    register(|r| {
        EventRegistry::register(
            &mut r.actions,
            owner,
            callback.tag.as_str(),
            callback.report,
            callback,
            orderings,
        )
    })
}
pub(crate) fn register_filter(
    owner: &str,
    callback: FilterCallback,
    orderings: &[HandlerOrdering],
) -> ApiResult<()> {
    register(|r| {
        EventRegistry::register(
            &mut r.filters,
            owner,
            callback.tag.as_str(),
            callback.report,
            callback,
            orderings,
        )
    })
}
pub(crate) fn freeze() -> Result<(), String> {
    let mut state = EVENTS.write();
    if state.frozen.is_some() {
        return Ok(());
    }
    let pending = state
        .pending
        .as_mut()
        .ok_or("extension hook registry unavailable")?;
    pending.freeze()?;
    state.frozen = state.pending.take().map(Arc::new);
    Ok(())
}
fn snapshot() -> Result<Arc<EventRegistry>, ApiString> {
    let state = EVENTS.read();
    state
        .frozen
        .as_ref()
        .map(Arc::clone)
        .ok_or_else(|| "extension hooks are unavailable during pre-init".into())
}
pub(crate) fn do_action(tag: &str, value: ActionValue<'_>) -> ApiResult<()> {
    snapshot()
        .and_then(|events| events.do_action(tag, value))
        .into()
}
pub(crate) fn apply_filters(tag: &str, value: FilterValue<'_>) -> ApiResult<()> {
    snapshot()
        .and_then(|events| events.apply_filters(tag, value))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{Filter, OrderingConstraint};
    #[stabby::stabby]
    struct Number {
        value: u32,
    }
    impl Filter for Number {
        const TAG: &'static str = "test:ordered_number";
    }
    extern "C" fn first(value: &mut Number) {
        value.value = value.value * 10 + 1;
    }
    extern "C" fn second(value: &mut Number) {
        value.value = value.value * 10 + 2;
    }

    #[test]
    fn filter_chain_applies_resolved_order_to_one_exclusive_payload() -> Result<(), String> {
        let mut registry = EventRegistry::default();
        let callback = FilterCallback::new(second);
        EventRegistry::register(
            &mut registry.filters,
            "second",
            Number::TAG,
            callback.report,
            callback,
            &[],
        )?;
        let callback = FilterCallback::new(first);
        EventRegistry::register(
            &mut registry.filters,
            "first",
            Number::TAG,
            callback.report,
            callback,
            &[HandlerOrdering {
                constraint: OrderingConstraint::Before,
                extension: "second".into(),
            }],
        )?;
        registry.freeze()?;
        let mut number = Number { value: 0 };
        registry
            .apply_filters(Number::TAG, FilterValue::new(&mut number))
            .map_err(|e| e.to_string())?;
        assert_eq!(number.value, 12);
        Ok(())
    }
}

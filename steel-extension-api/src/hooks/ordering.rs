use super::{HandlerOrdering, OrderingConstraint};
use std::collections::BTreeSet;

pub(super) fn resolve(handlers: &[(&str, &[HandlerOrdering])]) -> Result<Vec<usize>, String> {
    let mut owners = Vec::new();
    for (owner, _) in handlers {
        if !owners.contains(owner) {
            owners.push(*owner);
        }
    }
    let mut edges = vec![BTreeSet::new(); owners.len()];
    let mut incoming = vec![0; owners.len()];
    for (index, owner) in owners.iter().enumerate() {
        for (_, orderings) in handlers.iter().filter(|(candidate, _)| candidate == owner) {
            for ordering in *orderings {
                let Some(target) = owners
                    .iter()
                    .position(|candidate| *candidate == ordering.extension.as_str())
                else {
                    continue;
                };
                let (from, to) = match ordering.constraint {
                    OrderingConstraint::Before => (index, target),
                    OrderingConstraint::After => (target, index),
                };
                if edges[from].insert(to) {
                    incoming[to] += 1;
                }
            }
        }
    }
    let mut ready: BTreeSet<_> = incoming
        .iter()
        .enumerate()
        .filter_map(|(index, &count)| (count == 0).then_some(index))
        .collect();
    let mut sorted = Vec::with_capacity(handlers.len());
    while let Some(index) = ready.pop_first() {
        sorted.push(index);
        for &target in &edges[index] {
            incoming[target] -= 1;
            if incoming[target] == 0 {
                ready.insert(target);
            }
        }
    }
    if sorted.len() != owners.len() {
        let involved: BTreeSet<_> = incoming
            .iter()
            .enumerate()
            .filter_map(|(index, &count)| (count > 0).then_some(owners[index]))
            .collect();
        return Err(format!("cyclic handler ordering involving {involved:?}"));
    }
    let owners = &owners;
    Ok(sorted
        .into_iter()
        .flat_map(|index| {
            handlers
                .iter()
                .enumerate()
                .filter_map(move |(handler, (owner, _))| {
                    (*owner == owners[index]).then_some(handler)
                })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn before(name: &str) -> HandlerOrdering {
        HandlerOrdering {
            constraint: OrderingConstraint::Before,
            extension: name.into(),
        }
    }
    #[test]
    fn resolves_constraints_stably_and_includes_all_handlers_of_an_owner() {
        let constraints = [before("second"), before("second")];
        let absent = [before("absent")];
        assert_eq!(
            resolve(&[
                ("second", &[]),
                ("first", &constraints),
                ("second", &[]),
                ("last", &absent)
            ]),
            Ok(vec![1, 0, 2, 3])
        );
    }
    #[test]
    fn rejects_cycles_and_self_dependencies() {
        assert!(resolve(&[("one", &[before("two")]), ("two", &[before("one")])]).is_err());
        assert!(resolve(&[("one", &[before("one")])]).is_err());
    }

    #[test]
    fn owner_constraints_keep_an_extensions_handlers_together_in_declaration_order() {
        let constraint = [before("second")];
        assert_eq!(
            resolve(&[("first", &[]), ("second", &[]), ("first", &constraint)]),
            Ok(vec![0, 2, 1])
        );
    }
}

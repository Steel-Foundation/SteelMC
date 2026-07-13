//! Vanilla-compatible NBT helpers.

mod codec;
mod path;
mod snbt;

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};

pub use codec::NbtNumeric;
pub use path::{
    NbtPath, NbtPathError, NbtPathErrorKind, NbtPathMutationError, parse_nbt_path,
    parse_nbt_path_argument,
};
pub use snbt::{
    SnbtError, SnbtErrorKind, SnbtNumberType, parse_snbt, parse_snbt_argument, parse_snbt_compound,
    parse_snbt_compound_argument,
};

/// Mirrors vanilla `NbtUtils.compareNbt`.
#[must_use]
pub fn compare_nbt(
    expected: Option<&NbtTag>,
    actual: Option<&NbtTag>,
    partial_list_matches: bool,
) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let Some(actual) = actual else {
        return false;
    };

    match (expected, actual) {
        (NbtTag::Compound(expected), NbtTag::Compound(actual)) => {
            compare_nbt_compounds(expected, actual, partial_list_matches)
        }
        (NbtTag::List(expected), NbtTag::List(actual)) if partial_list_matches => {
            compare_lists_partially(expected, actual)
        }
        _ => expected == actual,
    }
}

/// Compares two compounds with vanilla's partial compound semantics.
#[must_use]
pub fn compare_nbt_compounds(
    expected: &NbtCompound,
    actual: &NbtCompound,
    partial_list_matches: bool,
) -> bool {
    if actual.len() < expected.len() {
        return false;
    }

    expected.iter().all(|(key, expected_tag)| {
        compare_nbt(
            Some(expected_tag),
            actual.get(&key.to_str()),
            partial_list_matches,
        )
    })
}

fn compare_lists_partially(expected: &NbtList, actual: &NbtList) -> bool {
    let expected = list_as_tags(expected);
    let actual = list_as_tags(actual);
    if expected.is_empty() {
        return actual.is_empty();
    }
    if actual.len() < expected.len() {
        return false;
    }

    expected.iter().all(|expected_tag| {
        actual
            .iter()
            .any(|actual_tag| compare_nbt(Some(expected_tag), Some(actual_tag), true))
    })
}

fn list_as_tags(list: &NbtList) -> Vec<NbtTag> {
    list.as_nbt_tags()
        .into_iter()
        .map(unwrap_list_wrapper)
        .collect()
}

fn unwrap_list_wrapper(tag: NbtTag) -> NbtTag {
    match tag {
        NbtTag::Compound(mut compound) if compound.len() == 1 && compound.contains("") => {
            let Some(value) = compound.take("") else {
                return NbtTag::Compound(compound);
            };
            value
        }
        tag => tag,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compound(entries: impl IntoIterator<Item = (&'static str, NbtTag)>) -> NbtTag {
        let mut compound = NbtCompound::new();
        for (key, tag) in entries {
            compound.insert(key, tag);
        }
        NbtTag::Compound(compound)
    }

    fn list(entries: impl IntoIterator<Item = NbtTag>) -> NbtTag {
        NbtTag::List(NbtList::from(entries.into_iter().collect::<Vec<_>>()))
    }

    #[test]
    fn compounds_and_lists_match_partially() {
        let expected = compound([(
            "values",
            list([compound([("name", NbtTag::String("second".into()))])]),
        )]);
        let actual = compound([(
            "values",
            list([
                compound([("name", NbtTag::String("first".into()))]),
                compound([
                    ("name", NbtTag::String("second".into())),
                    ("extra", NbtTag::Byte(1)),
                ]),
            ]),
        )]);

        assert!(compare_nbt(Some(&expected), Some(&actual), true));
        assert!(!compare_nbt(Some(&expected), Some(&actual), false));
    }

    #[test]
    fn empty_partial_list_only_matches_an_empty_list() {
        let empty = list([]);
        let non_empty = list([NbtTag::Int(1)]);

        assert!(compare_nbt(Some(&empty), Some(&empty), true));
        assert!(!compare_nbt(Some(&empty), Some(&non_empty), true));
    }

    #[test]
    fn partial_lists_match_heterogeneous_values() {
        let expected = list([NbtTag::String("two".into())]);
        let actual = list([NbtTag::Int(1), NbtTag::String("two".into())]);

        assert!(compare_nbt(Some(&expected), Some(&actual), true));
    }

    #[test]
    fn scalar_tags_require_the_same_nbt_type() {
        assert!(compare_nbt(
            Some(&NbtTag::Int(1)),
            Some(&NbtTag::Int(1)),
            true
        ));
        assert!(!compare_nbt(
            Some(&NbtTag::Int(1)),
            Some(&NbtTag::Long(1)),
            true
        ));
    }
}

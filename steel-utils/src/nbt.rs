//! Vanilla-compatible NBT helpers.

mod codec;
mod path;
mod snbt;

use rustc_hash::FxHashSet;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};

pub use codec::NbtNumeric;
pub use path::{
    NbtPath, NbtPathError, NbtPathErrorKind, NbtPathMutationError, parse_nbt_path,
    parse_nbt_path_argument,
};
pub use snbt::{
    SnbtError, SnbtErrorKind, SnbtNumberType, parse_snbt, parse_snbt_argument, parse_snbt_compound,
    parse_snbt_compound_argument, to_canonical_snbt,
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
        (NbtTag::Compound(expected), NbtTag::Compound(actual)) if partial_list_matches => {
            compare_nbt_compounds(expected, actual, partial_list_matches)
        }
        (NbtTag::List(expected), NbtTag::List(actual)) if partial_list_matches => {
            compare_lists_partially(expected, actual)
        }
        _ => nbt_tags_equal(expected, actual),
    }
}

/// Compares NBT values with Vanilla's `Tag.equals` semantics.
///
/// Compounds are maps rather than ordered entry lists. Floating-point tags
/// canonicalize NaNs while still distinguishing positive and negative zero.
#[must_use]
pub fn nbt_tags_equal(left: &NbtTag, right: &NbtTag) -> bool {
    match (left, right) {
        (NbtTag::Byte(left), NbtTag::Byte(right)) => left == right,
        (NbtTag::Short(left), NbtTag::Short(right)) => left == right,
        (NbtTag::Int(left), NbtTag::Int(right)) => left == right,
        (NbtTag::Long(left), NbtTag::Long(right)) => left == right,
        (NbtTag::Float(left), NbtTag::Float(right)) => float_tags_equal(*left, *right),
        (NbtTag::Double(left), NbtTag::Double(right)) => double_tags_equal(*left, *right),
        (NbtTag::ByteArray(left), NbtTag::ByteArray(right)) => left == right,
        (NbtTag::String(left), NbtTag::String(right)) => left == right,
        (NbtTag::List(left), NbtTag::List(right)) => {
            let left = nbt_list_values(left);
            let right = nbt_list_values(right);
            left.len() == right.len()
                && left
                    .iter()
                    .zip(&right)
                    .all(|(left, right)| nbt_tags_equal(left, right))
        }
        (NbtTag::Compound(left), NbtTag::Compound(right)) => nbt_compounds_equal(left, right),
        (NbtTag::IntArray(left), NbtTag::IntArray(right)) => left == right,
        (NbtTag::LongArray(left), NbtTag::LongArray(right)) => left == right,
        _ => false,
    }
}

/// Compares compounds as Vanilla maps, independent of serialized entry order.
#[must_use]
pub fn nbt_compounds_equal(left: &NbtCompound, right: &NbtCompound) -> bool {
    left.len() == right.len()
        && left.iter().all(|(key, left_value)| {
            right
                .get(&key.to_str())
                .is_some_and(|right_value| nbt_tags_equal(left_value, right_value))
        })
}

const fn float_tags_equal(left: f32, right: f32) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

const fn double_tags_equal(left: f64, right: f64) -> bool {
    (left.is_nan() && right.is_nan()) || left.to_bits() == right.to_bits()
}

/// Converts an NBT compound to Vanilla's map representation.
///
/// Vanilla rejects malformed modified UTF-8 and keeps the final occurrence of
/// a duplicate compound key. `simdnbt` preserves raw strings and duplicates,
/// so codecs crossing a trust boundary normalize them explicitly.
#[must_use]
pub fn normalize_nbt_compound(compound: NbtCompound) -> Option<NbtCompound> {
    let mut normalized = NbtCompound::new();
    for (key, value) in compound {
        let key = key.try_into_string().ok()?;
        let value = normalize_nbt_tag(value)?;
        while normalized.remove(&key).is_some() {}
        normalized.insert(key, value);
    }
    Some(normalized)
}

/// Converts an NBT value and all descendants to Vanilla's canonical in-memory
/// representation.
pub fn normalize_nbt_tag(tag: NbtTag) -> Option<NbtTag> {
    match tag {
        NbtTag::String(value) => Some(NbtTag::String(value.try_into_string().ok()?.into())),
        NbtTag::List(list) => normalize_nbt_list(list).map(NbtTag::List),
        NbtTag::Compound(compound) => normalize_nbt_compound(compound).map(NbtTag::Compound),
        tag => Some(tag),
    }
}

fn normalize_nbt_list(list: NbtList) -> Option<NbtList> {
    match list {
        NbtList::String(values) => values
            .into_iter()
            .map(|value| value.try_into_string().ok().map(Into::into))
            .collect::<Option<Vec<_>>>()
            .map(NbtList::String),
        NbtList::List(values) => values
            .into_iter()
            .map(normalize_nbt_list)
            .collect::<Option<Vec<_>>>()
            .map(NbtList::List),
        NbtList::Compound(values) => values
            .into_iter()
            .map(normalize_nbt_compound)
            .collect::<Option<Vec<_>>>()
            .map(NbtList::Compound),
        list => Some(list),
    }
}

/// Recursively applies Vanilla `CompoundTag.merge` semantics.
pub fn merge_nbt_compounds(target: &mut NbtCompound, source: &NbtCompound) {
    for (key, source_value) in source.iter() {
        let key = key.to_str();
        if let NbtTag::Compound(source_compound) = source_value
            && let Some(NbtTag::Compound(target_compound)) = target.get_mut(&key)
        {
            merge_nbt_compounds(target_compound, source_compound);
            continue;
        }

        while target.remove(&key).is_some() {}
        target.insert(key.into_owned(), source_value.clone());
    }
}

/// Returns the heap usage charged by Vanilla's `NbtAccounter` while decoding.
///
/// `None` indicates malformed modified UTF-8 or arithmetic overflow.
#[must_use]
pub fn vanilla_nbt_heap_size(tag: &NbtTag) -> Option<u64> {
    match tag {
        NbtTag::Byte(_) => Some(9),
        NbtTag::Short(_) => Some(10),
        NbtTag::Int(_) | NbtTag::Float(_) => Some(12),
        NbtTag::Long(_) | NbtTag::Double(_) => Some(16),
        NbtTag::ByteArray(values) => sized_array(24, 1, values.len()),
        NbtTag::String(value) => sized_string(36, value),
        NbtTag::List(values) => vanilla_nbt_list_heap_size(values),
        NbtTag::Compound(values) => vanilla_nbt_compound_heap_size(values),
        NbtTag::IntArray(values) => sized_array(24, 4, values.len()),
        NbtTag::LongArray(values) => sized_array(24, 8, values.len()),
    }
}

fn vanilla_nbt_list_heap_size(list: &NbtList) -> Option<u64> {
    let values = list.as_nbt_tags();
    let count = u64::try_from(values.len()).ok()?;
    let mut size = 36_u64.checked_add(4_u64.checked_mul(count)?)?;
    for value in &values {
        size = size.checked_add(vanilla_nbt_heap_size(value)?)?;
    }
    Some(size)
}

fn vanilla_nbt_compound_heap_size(compound: &NbtCompound) -> Option<u64> {
    let mut size = 48_u64;
    let mut keys = FxHashSet::default();
    for (key, value) in compound.iter() {
        let key = key.to_owned().try_into_string().ok()?;
        let key_units = u64::try_from(key.encode_utf16().count()).ok()?;
        size = size.checked_add(28_u64.checked_add(2_u64.checked_mul(key_units)?)?)?;
        size = size.checked_add(vanilla_nbt_heap_size(value)?)?;
        if keys.insert(key) {
            size = size.checked_add(36)?;
        }
    }
    Some(size)
}

fn sized_string(base: u64, value: &simdnbt::Mutf8Str) -> Option<u64> {
    let value = value.to_owned().try_into_string().ok()?;
    let units = u64::try_from(value.encode_utf16().count()).ok()?;
    base.checked_add(2_u64.checked_mul(units)?)
}

fn sized_array(base: u64, element_size: u64, len: usize) -> Option<u64> {
    let len = u64::try_from(len).ok()?;
    base.checked_add(element_size.checked_mul(len)?)
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
    let expected = nbt_list_values(expected);
    let actual = nbt_list_values(actual);
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

/// Returns the semantic values of a Vanilla list.
///
/// Vanilla 26.2 permits heterogeneous lists and wraps non-compound elements
/// when serializing them through the homogeneous binary NBT format.
#[must_use]
pub fn nbt_list_values(list: &NbtList) -> Vec<NbtTag> {
    list.as_nbt_tags()
        .into_iter()
        .map(unwrap_list_wrapper)
        .collect()
}

/// Returns values exposed by Vanilla's `NbtOps` collection interface.
///
/// Numeric arrays are collections as well as ordinary list tags, so codecs
/// built with `Codec.listOf()` accept all four representations.
#[must_use]
pub fn nbt_collection_values(tag: &NbtTag) -> Option<Vec<NbtTag>> {
    match tag {
        NbtTag::ByteArray(values) => Some(
            values
                .iter()
                .map(|value| NbtTag::Byte(i8::from_ne_bytes([*value])))
                .collect(),
        ),
        NbtTag::List(values) => Some(nbt_list_values(values)),
        NbtTag::IntArray(values) => Some(values.iter().copied().map(NbtTag::Int).collect()),
        NbtTag::LongArray(values) => Some(values.iter().copied().map(NbtTag::Long).collect()),
        _ => None,
    }
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

    #[test]
    fn exact_equality_matches_vanilla_map_and_float_rules() {
        let left = compound([
            ("first", NbtTag::Int(1)),
            ("second", NbtTag::Float(f32::from_bits(0x7fc0_0001))),
        ]);
        let right = compound([
            ("second", NbtTag::Float(f32::from_bits(0x7fc0_0002))),
            ("first", NbtTag::Int(1)),
        ]);

        assert!(nbt_tags_equal(&left, &right));
        assert!(!nbt_tags_equal(&NbtTag::Float(0.0), &NbtTag::Float(-0.0)));
    }

    #[test]
    fn normalization_keeps_the_final_duplicate_key() {
        let mut raw = NbtCompound::new();
        raw.insert("value", 1);
        raw.insert("value", 2);

        let normalized = normalize_nbt_compound(raw).expect("valid compound should normalize");
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized.int("value"), Some(2));
    }

    #[test]
    fn compound_merge_recurses_only_when_both_values_are_compounds() {
        let mut nested = NbtCompound::new();
        nested.insert("kept", 1);
        nested.insert("replaced", 1);
        let mut target = NbtCompound::new();
        target.insert("nested", nested);
        target.insert("scalar", 1);

        let mut nested = NbtCompound::new();
        nested.insert("replaced", 2);
        let mut source = NbtCompound::new();
        source.insert("nested", nested);
        source.insert("scalar", NbtCompound::new());

        merge_nbt_compounds(&mut target, &source);
        let nested = target
            .compound("nested")
            .expect("nested compound should remain");
        assert_eq!(nested.int("kept"), Some(1));
        assert_eq!(nested.int("replaced"), Some(2));
        assert!(target.compound("scalar").is_some());
    }

    #[test]
    fn heap_size_matches_vanilla_nbt_accounter_formulas() {
        let mut raw = NbtCompound::new();
        raw.insert("a", 1);
        raw.insert("a", 2);

        assert_eq!(vanilla_nbt_heap_size(&NbtTag::Compound(raw)), Some(168));
    }
}

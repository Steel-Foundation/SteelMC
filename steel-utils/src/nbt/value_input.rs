//! Lenient numeric field access mirroring vanilla's `ValueInput`.
//!
//! Vanilla reads numeric save fields through `TagValueInput.getNumericTag`,
//! which accepts *any* numeric tag (byte, short, int, long, float, double) and
//! converts it with the `NumericTag.byteValue()`/`intValue()`/... conversions.
//! A `Color: 3s` short is therefore read as byte `3`, and `Sheared: 0.7d` as
//! byte `0` (false). The raw `simdnbt` accessors only accept the exact tag type,
//! so entity and block entity loaders should go through [`NbtValueInput`].

use simdnbt::{
    borrow::{NbtCompound as BorrowedNbtCompound, NbtTag as BorrowedNbtTag},
    owned::{NbtCompound as OwnedNbtCompound, NbtTag as OwnedNbtTag},
};

/// Conversions performed by vanilla's `NumericTag` implementations.
///
/// Integer tags truncate to narrower integers (two's complement wraparound)
/// and cast to floats. Float and double tags floor before converting to
/// `byte`/`short`/`int`; `FloatTag.longValue()` truncates while
/// `DoubleTag.longValue()` floors, exactly like vanilla.
pub trait NbtNumericTag {
    /// `NumericTag.byteValue() != 0`, which is how vanilla decodes booleans.
    fn numeric_bool(&self) -> Option<bool> {
        self.numeric_i8().map(|value| value != 0)
    }

    /// Mirrors `NumericTag.byteValue()`.
    fn numeric_i8(&self) -> Option<i8>;

    /// Mirrors `NumericTag.shortValue()`.
    fn numeric_i16(&self) -> Option<i16>;

    /// Mirrors `NumericTag.intValue()`.
    fn numeric_i32(&self) -> Option<i32>;

    /// Mirrors `NumericTag.longValue()`.
    fn numeric_i64(&self) -> Option<i64>;

    /// Mirrors `NumericTag.floatValue()`.
    fn numeric_f32(&self) -> Option<f32>;

    /// Mirrors `NumericTag.doubleValue()`.
    fn numeric_f64(&self) -> Option<f64>;
}

/// A numeric NBT tag lifted out of either the owned or borrowed representation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NbtNumber {
    /// `TAG_Byte`.
    Byte(i8),
    /// `TAG_Short`.
    Short(i16),
    /// `TAG_Int`.
    Int(i32),
    /// `TAG_Long`.
    Long(i64),
    /// `TAG_Float`.
    Float(f32),
    /// `TAG_Double`.
    Double(f64),
}

impl NbtNumber {
    /// Extracts the numeric payload of an owned tag, if it has one.
    #[must_use]
    pub const fn from_owned(tag: &OwnedNbtTag) -> Option<Self> {
        match tag {
            OwnedNbtTag::Byte(value) => Some(Self::Byte(*value)),
            OwnedNbtTag::Short(value) => Some(Self::Short(*value)),
            OwnedNbtTag::Int(value) => Some(Self::Int(*value)),
            OwnedNbtTag::Long(value) => Some(Self::Long(*value)),
            OwnedNbtTag::Float(value) => Some(Self::Float(*value)),
            OwnedNbtTag::Double(value) => Some(Self::Double(*value)),
            _ => None,
        }
    }

    /// Extracts the numeric payload of a borrowed tag, if it has one.
    #[must_use]
    pub fn from_borrowed(tag: &BorrowedNbtTag<'_, '_>) -> Option<Self> {
        tag.byte()
            .map(Self::Byte)
            .or_else(|| tag.short().map(Self::Short))
            .or_else(|| tag.int().map(Self::Int))
            .or_else(|| tag.long().map(Self::Long))
            .or_else(|| tag.float().map(Self::Float))
            .or_else(|| tag.double().map(Self::Double))
    }
}

impl NbtNumericTag for NbtNumber {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla NumericTag conversions wrap/saturate"
    )]
    fn numeric_i8(&self) -> Option<i8> {
        Some(match *self {
            Self::Byte(value) => value,
            Self::Short(value) => value as i8,
            Self::Int(value) => value as i8,
            Self::Long(value) => value as i8,
            Self::Float(value) => (value.floor() as i32) as i8,
            Self::Double(value) => (value.floor() as i32) as i8,
        })
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla NumericTag conversions wrap/saturate"
    )]
    fn numeric_i16(&self) -> Option<i16> {
        Some(match *self {
            Self::Byte(value) => i16::from(value),
            Self::Short(value) => value,
            Self::Int(value) => value as i16,
            Self::Long(value) => value as i16,
            Self::Float(value) => (value.floor() as i32) as i16,
            Self::Double(value) => (value.floor() as i32) as i16,
        })
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla NumericTag conversions wrap/saturate"
    )]
    fn numeric_i32(&self) -> Option<i32> {
        Some(match *self {
            Self::Byte(value) => i32::from(value),
            Self::Short(value) => i32::from(value),
            Self::Int(value) => value,
            Self::Long(value) => value as i32,
            Self::Float(value) => value.floor() as i32,
            Self::Double(value) => value.floor() as i32,
        })
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla NumericTag conversions wrap/saturate"
    )]
    fn numeric_i64(&self) -> Option<i64> {
        Some(match *self {
            Self::Byte(value) => i64::from(value),
            Self::Short(value) => i64::from(value),
            Self::Int(value) => i64::from(value),
            Self::Long(value) => value,
            // `FloatTag.longValue()` is a plain `(long)` cast (truncation) ...
            Self::Float(value) => value as i64,
            // ... while `DoubleTag.longValue()` floors first.
            Self::Double(value) => value.floor() as i64,
        })
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "vanilla NumericTag conversions lose precision"
    )]
    fn numeric_f32(&self) -> Option<f32> {
        Some(match *self {
            Self::Byte(value) => f32::from(value),
            Self::Short(value) => f32::from(value),
            Self::Int(value) => value as f32,
            Self::Long(value) => value as f32,
            Self::Float(value) => value,
            Self::Double(value) => value as f32,
        })
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "vanilla NumericTag conversions lose precision"
    )]
    fn numeric_f64(&self) -> Option<f64> {
        Some(match *self {
            Self::Byte(value) => f64::from(value),
            Self::Short(value) => f64::from(value),
            Self::Int(value) => f64::from(value),
            Self::Long(value) => value as f64,
            Self::Float(value) => f64::from(value),
            Self::Double(value) => value,
        })
    }
}

macro_rules! forward_numeric_tag {
    ($ty:ty, |$tag:ident| $lift:expr) => {
        impl NbtNumericTag for $ty {
            fn numeric_i8(&self) -> Option<i8> {
                let $tag = self;
                $lift.and_then(|number| number.numeric_i8())
            }
            fn numeric_i16(&self) -> Option<i16> {
                let $tag = self;
                $lift.and_then(|number| number.numeric_i16())
            }
            fn numeric_i32(&self) -> Option<i32> {
                let $tag = self;
                $lift.and_then(|number| number.numeric_i32())
            }
            fn numeric_i64(&self) -> Option<i64> {
                let $tag = self;
                $lift.and_then(|number| number.numeric_i64())
            }
            fn numeric_f32(&self) -> Option<f32> {
                let $tag = self;
                $lift.and_then(|number| number.numeric_f32())
            }
            fn numeric_f64(&self) -> Option<f64> {
                let $tag = self;
                $lift.and_then(|number| number.numeric_f64())
            }
        }
    };
}

forward_numeric_tag!(OwnedNbtTag, |tag| NbtNumber::from_owned(tag));
forward_numeric_tag!(BorrowedNbtTag<'_, '_>, |tag| NbtNumber::from_borrowed(tag));

/// Field accessors mirroring vanilla's `ValueInput.get*Or` family.
///
/// Every method accepts any numeric tag type and converts it like
/// [`NbtNumericTag`]; non-numeric or missing tags yield `None` / the default.
pub trait NbtValueInput {
    /// Looks up `name` and returns its numeric payload, if it is a numeric tag.
    fn get_number(&self, name: &str) -> Option<NbtNumber>;

    /// Mirrors `ValueInput.getBoolean`.
    fn get_bool(&self, name: &str) -> Option<bool> {
        self.get_number(name)
            .and_then(|number| number.numeric_bool())
    }

    /// Mirrors `ValueInput.getBooleanOr`.
    fn get_bool_or(&self, name: &str, default: bool) -> bool {
        self.get_bool(name).unwrap_or(default)
    }

    /// Mirrors `ValueInput.getByte`.
    fn get_i8(&self, name: &str) -> Option<i8> {
        self.get_number(name).and_then(|number| number.numeric_i8())
    }

    /// Mirrors `ValueInput.getByteOr`.
    fn get_i8_or(&self, name: &str, default: i8) -> i8 {
        self.get_i8(name).unwrap_or(default)
    }

    /// Mirrors `ValueInput.getShort`.
    fn get_i16(&self, name: &str) -> Option<i16> {
        self.get_number(name)
            .and_then(|number| number.numeric_i16())
    }

    /// Mirrors `ValueInput.getShortOr`.
    fn get_i16_or(&self, name: &str, default: i16) -> i16 {
        self.get_i16(name).unwrap_or(default)
    }

    /// Mirrors `ValueInput.getInt`.
    fn get_i32(&self, name: &str) -> Option<i32> {
        self.get_number(name)
            .and_then(|number| number.numeric_i32())
    }

    /// Mirrors `ValueInput.getIntOr`.
    fn get_i32_or(&self, name: &str, default: i32) -> i32 {
        self.get_i32(name).unwrap_or(default)
    }

    /// Mirrors `ValueInput.getLong`.
    fn get_i64(&self, name: &str) -> Option<i64> {
        self.get_number(name)
            .and_then(|number| number.numeric_i64())
    }

    /// Mirrors `ValueInput.getLongOr`.
    fn get_i64_or(&self, name: &str, default: i64) -> i64 {
        self.get_i64(name).unwrap_or(default)
    }

    /// Mirrors `ValueInput.getFloat`.
    fn get_f32(&self, name: &str) -> Option<f32> {
        self.get_number(name)
            .and_then(|number| number.numeric_f32())
    }

    /// Mirrors `ValueInput.getFloatOr`.
    fn get_f32_or(&self, name: &str, default: f32) -> f32 {
        self.get_f32(name).unwrap_or(default)
    }

    /// Mirrors `ValueInput.getDouble`.
    fn get_f64(&self, name: &str) -> Option<f64> {
        self.get_number(name)
            .and_then(|number| number.numeric_f64())
    }

    /// Mirrors `ValueInput.getDoubleOr`.
    fn get_f64_or(&self, name: &str, default: f64) -> f64 {
        self.get_f64(name).unwrap_or(default)
    }
}

impl NbtValueInput for OwnedNbtCompound {
    fn get_number(&self, name: &str) -> Option<NbtNumber> {
        self.get(name).and_then(NbtNumber::from_owned)
    }
}

impl NbtValueInput for BorrowedNbtCompound<'_, '_> {
    fn get_number(&self, name: &str) -> Option<NbtNumber> {
        self.get(name)
            .and_then(|tag| NbtNumber::from_borrowed(&tag))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::{
        borrow::read as read_borrowed,
        owned::{BaseNbt, NbtCompound, NbtTag},
    };

    use super::{NbtNumericTag, NbtValueInput};

    fn compound() -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.insert("byte", 3_i8);
        nbt.insert("short", 300_i16);
        nbt.insert("int", 70_000_i32);
        nbt.insert("long", i64::from(i32::MAX) + 1);
        nbt.insert("float", 0.7_f32);
        nbt.insert("double", -0.5_f64);
        nbt.insert("string", "1");
        nbt
    }

    #[test]
    fn any_numeric_tag_is_accepted() {
        let nbt = compound();
        assert_eq!(nbt.get_i8("short"), Some(300_i16 as i8));
        assert_eq!(nbt.get_i16("int"), Some(70_000_i32 as i16));
        assert_eq!(nbt.get_i32("long"), Some(i32::MIN));
        assert_eq!(nbt.get_i64("byte"), Some(3));
        assert_eq!(nbt.get_f32("double"), Some(-0.5));
        assert_eq!(nbt.get_f64("int"), Some(70_000.0));
    }

    #[test]
    fn booleans_use_byte_value() {
        let nbt = compound();
        assert_eq!(nbt.get_bool("byte"), Some(true));
        assert_eq!(nbt.get_bool("short"), Some(true));
        // 0.7 floors to 0.
        assert_eq!(nbt.get_bool("float"), Some(false));
        // -0.5 floors to -1.
        assert_eq!(nbt.get_bool("double"), Some(true));
        // 256 truncates to 0 as a byte.
        assert_eq!(NbtTag::Int(256).numeric_bool(), Some(false));
        assert!(nbt.get_bool_or("missing", true));
    }

    #[test]
    fn floating_conversions_match_vanilla() {
        assert_eq!(NbtTag::Double(-0.5).numeric_i32(), Some(-1));
        assert_eq!(NbtTag::Float(-0.5).numeric_i32(), Some(-1));
        assert_eq!(NbtTag::Double(-0.5).numeric_i64(), Some(-1));
        // FloatTag.longValue() truncates instead of flooring.
        assert_eq!(NbtTag::Float(-0.5).numeric_i64(), Some(0));
        assert_eq!(NbtTag::Double(1e10).numeric_i32(), Some(i32::MAX));
        assert_eq!(NbtTag::Double(f64::NAN).numeric_i32(), Some(0));
    }

    #[test]
    fn non_numeric_and_missing_tags_fall_back() {
        let nbt = compound();
        assert_eq!(nbt.get_i32("string"), None);
        assert_eq!(nbt.get_i32("missing"), None);
        assert_eq!(nbt.get_i32_or("string", 7), 7);
        assert_eq!(nbt.get_f64_or("missing", 1.5), 1.5);
    }

    #[test]
    fn borrowed_compounds_use_the_same_conversions() {
        let nbt = BaseNbt::new("", compound());
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed(&mut Cursor::new(bytes.as_slice()))
            .expect("owned test nbt should parse")
            .unwrap();
        let view = borrowed.as_compound();

        assert_eq!(view.get_i8("short"), Some(300_i16 as i8));
        assert_eq!(view.get_bool("float"), Some(false));
        assert_eq!(view.get_i64("long"), Some(i64::from(i32::MAX) + 1));
        assert_eq!(view.get_i32("string"), None);
        assert_eq!(view.get_i16_or("missing", 9), 9);
    }
}

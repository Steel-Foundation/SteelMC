//! Numeric coercions used by vanilla's NBT-backed codecs.

use simdnbt::{borrow::NbtTag as BorrowedNbtTag, owned::NbtTag as OwnedNbtTag};

/// Decodes numeric NBT tags with the conversions performed by vanilla's
/// `DynamicOps` number codecs.
pub trait NbtNumeric {
    /// Decodes `Codec.BOOL` from any numeric NBT tag.
    fn codec_bool(&self) -> Option<bool>;

    /// Decodes `Codec.INT` from any numeric NBT tag.
    fn codec_i32(&self) -> Option<i32>;

    /// Decodes `Codec.FLOAT` from any numeric NBT tag.
    fn codec_f32(&self) -> Option<f32>;

    /// Decodes `Codec.DOUBLE` from any numeric NBT tag.
    fn codec_f64(&self) -> Option<f64>;
}

impl NbtNumeric for OwnedNbtTag {
    fn codec_bool(&self) -> Option<bool> {
        match self {
            Self::Byte(value) => Some(*value != 0),
            Self::Short(value) => Some(*value != 0),
            Self::Int(value) => Some(*value != 0),
            Self::Long(value) => Some(*value != 0),
            Self::Float(value) => Some(*value != 0.0),
            Self::Double(value) => Some(*value != 0.0),
            _ => None,
        }
    }

    fn codec_i32(&self) -> Option<i32> {
        match self {
            Self::Byte(value) => Some(i32::from(*value)),
            Self::Short(value) => Some(i32::from(*value)),
            Self::Int(value) => Some(*value),
            Self::Long(value) => Some(*value as i32),
            Self::Float(value) => Some(*value as i32),
            Self::Double(value) => Some(*value as i32),
            _ => None,
        }
    }

    fn codec_f32(&self) -> Option<f32> {
        match self {
            Self::Byte(value) => Some(f32::from(*value)),
            Self::Short(value) => Some(f32::from(*value)),
            Self::Int(value) => Some(*value as f32),
            Self::Long(value) => Some(*value as f32),
            Self::Float(value) => Some(*value),
            Self::Double(value) => Some(*value as f32),
            _ => None,
        }
    }

    fn codec_f64(&self) -> Option<f64> {
        match self {
            Self::Byte(value) => Some(f64::from(*value)),
            Self::Short(value) => Some(f64::from(*value)),
            Self::Int(value) => Some(f64::from(*value)),
            Self::Long(value) => Some(*value as f64),
            Self::Float(value) => Some(f64::from(*value)),
            Self::Double(value) => Some(*value),
            _ => None,
        }
    }
}

impl NbtNumeric for BorrowedNbtTag<'_, '_> {
    fn codec_bool(&self) -> Option<bool> {
        self.byte()
            .map(|value| value != 0)
            .or_else(|| self.short().map(|value| value != 0))
            .or_else(|| self.int().map(|value| value != 0))
            .or_else(|| self.long().map(|value| value != 0))
            .or_else(|| self.float().map(|value| value != 0.0))
            .or_else(|| self.double().map(|value| value != 0.0))
    }

    fn codec_i32(&self) -> Option<i32> {
        self.byte()
            .map(i32::from)
            .or_else(|| self.short().map(i32::from))
            .or_else(|| self.int())
            .or_else(|| self.long().map(|value| value as i32))
            .or_else(|| self.float().map(|value| value as i32))
            .or_else(|| self.double().map(|value| value as i32))
    }

    fn codec_f32(&self) -> Option<f32> {
        self.byte()
            .map(f32::from)
            .or_else(|| self.short().map(f32::from))
            .or_else(|| self.int().map(|value| value as f32))
            .or_else(|| self.long().map(|value| value as f32))
            .or_else(|| self.float())
            .or_else(|| self.double().map(|value| value as f32))
    }

    fn codec_f64(&self) -> Option<f64> {
        self.byte()
            .map(f64::from)
            .or_else(|| self.short().map(f64::from))
            .or_else(|| self.int().map(f64::from))
            .or_else(|| self.long().map(|value| value as f64))
            .or_else(|| self.float().map(f64::from))
            .or_else(|| self.double())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::{borrow::read_tag, owned::NbtTag};

    use super::NbtNumeric;

    #[test]
    fn owned_numeric_tags_use_codec_coercions() {
        assert_eq!(NbtTag::Double(5.5).codec_f32(), Some(5.5));
        assert_eq!(NbtTag::Float(5.9).codec_i32(), Some(5));
        assert_eq!(
            NbtTag::Long(i64::from(i32::MAX) + 1).codec_i32(),
            Some(i32::MIN)
        );
        assert_eq!(NbtTag::Double(f64::NAN).codec_bool(), Some(true));
        assert_eq!(NbtTag::String("1".into()).codec_f64(), None);
    }

    #[test]
    fn borrowed_numeric_tags_use_codec_coercions() {
        let tag = NbtTag::Double(5.5);
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed =
            read_tag(&mut Cursor::new(bytes.as_slice())).expect("owned test tag should parse");

        assert_eq!(borrowed.as_tag().codec_f32(), Some(5.5));
    }
}

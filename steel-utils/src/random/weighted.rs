//! Mirrors vanilla `net.minecraft.util.random.Weighted`.

use std::io::{Result, Write};

use crate::codec::VarInt;
use crate::serial::WriteTo;

/// A value paired with its selection weight.
///
/// Only the wire form is ported. Vanilla's `WeightedList` wrapper adds random selection
/// over a running total, which nothing needs yet, and a plain `Vec<Weighted<T>>` already
/// encodes identically: `WeightedList.streamCodec` is just `Weighted.streamCodec` under
/// `ByteBufCodecs.list()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weighted<T> {
    /// The value being weighted.
    pub value: T,
    /// How strongly this entry is favored relative to its siblings.
    pub weight: i32,
}

impl<T> Weighted<T> {
    /// The weight vanilla's `WeightedList.Builder::add` assumes when none is given.
    pub const DEFAULT_WEIGHT: i32 = 1;

    /// Pairs `value` with an explicit weight.
    #[must_use]
    pub const fn new(value: T, weight: i32) -> Self {
        Self { value, weight }
    }

    /// Pairs `value` with [`Self::DEFAULT_WEIGHT`].
    #[must_use]
    pub const fn unit(value: T) -> Self {
        Self::new(value, Self::DEFAULT_WEIGHT)
    }
}

impl<T: WriteTo> WriteTo for Weighted<T> {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.value.write(writer)?;
        VarInt(self.weight).write(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::Weighted;
    use crate::codec::VarInt;
    use crate::serial::WriteTo;

    #[test]
    fn a_weighted_value_writes_its_weight_after_the_value() {
        let weighted = Weighted::new(7_i32, 3);

        let mut encoded = Vec::new();
        weighted
            .write(&mut encoded)
            .expect("weighted should encode");

        let mut expected = 7_i32.to_be_bytes().to_vec();
        VarInt(3)
            .write(&mut expected)
            .expect("weight should encode");
        assert_eq!(encoded, expected);
    }

    #[test]
    fn an_unweighted_value_takes_vanillas_default() {
        assert_eq!(Weighted::unit(0_i32).weight, 1);
    }
}

use std::io::{Cursor, Result, Write};

use glam::DVec3;

use crate::{
    codec::VarInt,
    serial::{ReadFrom, WriteTo},
};

const DATA_BITS_MASK: u64 = 32_767;
const MAX_QUANTIZED_VALUE: f64 = 32_766.0;
const CONTINUATION_FLAG: u8 = 4;
const ABS_MAX_VALUE: f64 = 1.717_986_918_3E10;
const ABS_MIN_VALUE: f64 = 3.051_944_088_384_301E-5;

/// Vanilla `LpVec3` packed vector codec.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LpVec3(pub DVec3);

impl ReadFrom for LpVec3 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let lowest = u8::read(data)?;
        if lowest == 0 {
            return Ok(Self(DVec3::ZERO));
        }

        let middle = u8::read(data)?;
        let highest = u32::read(data)?;
        let buffer = (u64::from(highest) << 16) | (u64::from(middle) << 8) | u64::from(lowest);
        let mut scale = u64::from(lowest & 3);
        if has_continuation_bit(lowest) {
            scale |= u64::from(VarInt::read(data)?.0 as u32) << 2;
        }
        let scale = scale as f64;

        Ok(Self(DVec3::new(
            unpack(buffer >> 3) * scale,
            unpack(buffer >> 18) * scale,
            unpack(buffer >> 33) * scale,
        )))
    }
}

impl WriteTo for LpVec3 {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let x = sanitize(self.0.x);
        let y = sanitize(self.0.y);
        let z = sanitize(self.0.z);
        let chessboard_length = x.abs().max(y.abs().max(z.abs()));

        if chessboard_length < ABS_MIN_VALUE {
            return writer.write_all(&[0]);
        }

        let scale = chessboard_length.ceil() as i64;
        let is_partial = (scale & 3) != scale;
        let markers = if is_partial {
            (scale & 3) | i64::from(CONTINUATION_FLAG)
        } else {
            scale
        };
        let buffer = markers
            | (pack(x / scale as f64) << 3)
            | (pack(y / scale as f64) << 18)
            | (pack(z / scale as f64) << 33);

        writer.write_all(&[buffer as u8])?;
        writer.write_all(&[(buffer >> 8) as u8])?;
        writer.write_all(&((buffer >> 16) as u32).to_be_bytes())?;
        if is_partial {
            VarInt((scale >> 2) as i32).write(writer)?;
        }
        Ok(())
    }
}

const fn has_continuation_bit(value: u8) -> bool {
    (value & CONTINUATION_FLAG) == CONTINUATION_FLAG
}

fn sanitize(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-ABS_MAX_VALUE, ABS_MAX_VALUE)
    }
}

fn pack(value: f64) -> i64 {
    ((value * 0.5 + 0.5) * MAX_QUANTIZED_VALUE).round() as i64
}

fn unpack(value: u64) -> f64 {
    (value & DATA_BITS_MASK).min(MAX_QUANTIZED_VALUE as u64) as f64 * 2.0 / MAX_QUANTIZED_VALUE
        - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(value: DVec3) -> Result<DVec3> {
        let mut bytes = Vec::new();
        LpVec3(value).write(&mut bytes)?;
        Ok(LpVec3::read(&mut Cursor::new(&bytes))?.0)
    }

    #[test]
    fn zero_vector_is_single_zero_byte() -> Result<()> {
        let mut bytes = Vec::new();

        LpVec3(DVec3::ZERO).write(&mut bytes)?;

        assert_eq!(bytes, vec![0]);
        assert_eq!(LpVec3::read(&mut Cursor::new(&bytes))?.0, DVec3::ZERO);
        Ok(())
    }

    #[test]
    fn packed_vector_matches_vanilla_quantization() -> Result<()> {
        let decoded = round_trip(DVec3::new(0.25, 0.5, 0.75))?;

        assert!((decoded.x - 0.25).abs() < 0.0001);
        assert!((decoded.y - 0.5).abs() < 0.0001);
        assert!((decoded.z - 0.75).abs() < 0.0001);
        Ok(())
    }

    #[test]
    fn continuation_scale_round_trips_large_vector() -> Result<()> {
        let decoded = round_trip(DVec3::new(5.0, 0.0, -3.0))?;

        assert!((decoded.x - 5.0).abs() < 0.001);
        assert!(decoded.y.abs() < 0.001);
        assert!((decoded.z + 3.0).abs() < 0.001);
        Ok(())
    }
}

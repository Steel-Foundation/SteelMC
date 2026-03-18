use crate::serial::{ReadFrom, WriteTo};
use glam::{DVec3, IVec2, IVec3, Vec3};
use std::io::{Cursor, Result, Write};

#[allow(missing_docs)]
impl WriteTo for IVec2 {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.x.write(writer)?;
        self.y.write(writer)
    }
}

#[allow(missing_docs)]
impl ReadFrom for IVec2 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            x: i32::read(data)?,
            y: i32::read(data)?,
        })
    }
}

#[allow(missing_docs)]
impl WriteTo for IVec3 {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.x.write(writer)?;
        self.y.write(writer)?;
        self.z.write(writer)
    }
}

#[allow(missing_docs)]
impl ReadFrom for IVec3 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            x: i32::read(data)?,
            y: i32::read(data)?,
            z: i32::read(data)?,
        })
    }
}

#[allow(missing_docs)]
impl WriteTo for DVec3 {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.x.write(writer)?;
        self.y.write(writer)?;
        self.z.write(writer)
    }
}

#[allow(missing_docs)]
impl ReadFrom for DVec3 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            x: f64::read(data)?,
            y: f64::read(data)?,
            z: f64::read(data)?,
        })
    }
}

#[allow(missing_docs)]
impl WriteTo for Vec3 {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.x.write(writer)?;
        self.y.write(writer)?;
        self.z.write(writer)
    }
}

#[allow(missing_docs)]
impl ReadFrom for Vec3 {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            x: f32::read(data)?,
            y: f32::read(data)?,
            z: f32::read(data)?,
        })
    }
}

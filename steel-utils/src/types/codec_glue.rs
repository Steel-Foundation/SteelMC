use std::io::{self, Cursor, Write};

use simdnbt::owned::{NbtCompound, NbtTag};

use crate::{
    codec::VarInt,
    hash::{ComponentHasher, HashComponent},
    serial::{ReadFrom, WriteTo},
};

/// A placeholder type for unimplemented component values.
/// Unlike `()`, this is a distinct type that can have its own trait implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Todo;

impl WriteTo for Todo {
    fn write(&self, _writer: &mut impl Write) -> io::Result<()> {
        // Placeholder components write nothing
        Ok(())
    }
}

impl ReadFrom for Todo {
    fn read(_data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        // Placeholder components read nothing
        Ok(Todo)
    }
}

impl HashComponent for Todo {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        // Hash as empty value
        hasher.put_empty();
    }
}

impl simdnbt::ToNbtTag for Todo {
    fn to_nbt_tag(self) -> NbtTag {
        // Placeholder components serialize as empty compound
        NbtTag::Compound(NbtCompound::new())
    }
}

impl simdnbt::FromNbtTag for Todo {
    fn from_nbt_tag(_tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        // Placeholder components always deserialize successfully
        Some(Todo)
    }
}

/// A raw block state id. Using the registry this id can be derived into a block and it's current properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockStateId(pub u16);

impl WriteTo for BlockStateId {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        VarInt(i32::from(self.0)).write(writer)
    }
}

impl ReadFrom for BlockStateId {
    fn read(data: &mut Cursor<&[u8]>) -> io::Result<Self> {
        let id = VarInt::read(data)?.0;
        #[expect(
            clippy::cast_sign_loss,
            reason = "VarInt is validated upstream; block state IDs are non-negative"
        )]
        Ok(Self(id as u16))
    }
}

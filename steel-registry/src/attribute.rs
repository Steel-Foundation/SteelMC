use rustc_hash::FxHashMap;
use steel_utils::{
    Identifier,
    codec::VarInt,
    serial::{ReadFrom, WriteTo},
};

/// The operation type for an attribute modifier.
///
/// Matches vanilla `AttributeModifier.Operation`:
/// - `AddValue` (0): `total += amount`
/// - `AddMultipliedBase` (1): `total += base * amount`
/// - `AddMultipliedTotal` (2): `total *= 1 + amount`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
#[expect(
    clippy::enum_variant_names,
    reason = "matches vanilla `AttributeModifier.Operation` names"
)]
pub enum AttributeModifierOperation {
    AddValue = 0,
    AddMultipliedBase = 1,
    AddMultipliedTotal = 2,
}

impl AttributeModifierOperation {
    #[must_use]
    pub const fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::AddValue),
            1 => Some(Self::AddMultipliedBase),
            2 => Some(Self::AddMultipliedTotal),
            _ => None,
        }
    }

    #[must_use]
    pub const fn by_name(name: &str) -> Option<Self> {
        match name {
            "add_value" => Some(Self::AddValue),
            "add_multiplied_base" => Some(Self::AddMultipliedBase),
            "add_multiplied_total" => Some(Self::AddMultipliedTotal),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AddValue => "add_value",
            Self::AddMultipliedBase => "add_multiplied_base",
            Self::AddMultipliedTotal => "add_multiplied_total",
        }
    }
}

impl WriteTo for AttributeModifierOperation {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        VarInt(*self as i32).write(writer)
    }
}

impl ReadFrom for AttributeModifierOperation {
    fn read(data: &mut std::io::Cursor<&[u8]>) -> std::io::Result<Self> {
        let id = VarInt::read(data)?.0;
        Ok(Self::from_id(id).unwrap_or(Self::AddValue))
    }
}

/// Vanilla entity attribute definition
///
/// Unlike vanilla's separate `Attribute` / `RangedAttribute` hierarchy, we
/// fold min/max directly into the struct since every attribute is ranged
#[derive(Debug)]
pub struct Attribute {
    pub key: Identifier,
    pub translation_key: &'static str,
    pub default_value: f64,
    pub min_value: f64,
    pub max_value: f64,
    pub syncable: bool,
}

impl Attribute {
    /// Clamps a value to this attribute's valid range
    #[must_use]
    pub const fn sanitize_value(&self, value: f64) -> f64 {
        value.clamp(self.min_value, self.max_value)
    }
}

pub type AttributeRef = &'static Attribute;

pub struct AttributeRegistry {
    attributes_by_id: Vec<AttributeRef>,
    attributes_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl Default for AttributeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AttributeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            attributes_by_id: Vec::new(),
            attributes_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    /// Registers a new attribute
    pub fn register(&mut self, attribute: AttributeRef) {
        assert!(
            self.allows_registering,
            "Cannot register attributes after the registry has been frozen"
        );
        let idx = self.attributes_by_id.len();
        self.attributes_by_key.insert(attribute.key.clone(), idx);
        self.attributes_by_id.push(attribute);
    }

    /// Replaces an attribute at a given index
    #[must_use]
    pub fn replace(&mut self, attribute: AttributeRef, id: usize) -> bool {
        if id >= self.attributes_by_id.len() {
            return false;
        }
        self.attributes_by_id[id] = attribute;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, AttributeRef)> + '_ {
        self.attributes_by_id
            .iter()
            .enumerate()
            .map(|(id, &attr)| (id, attr))
    }
}

crate::impl_registry!(
    AttributeRegistry,
    Attribute,
    attributes_by_id,
    attributes_by_key,
    attributes
);
crate::impl_tagged_registry!(AttributeRegistry, attributes_by_key, "attribute");

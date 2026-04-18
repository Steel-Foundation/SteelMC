use rustc_hash::FxHashMap;
use steel_utils::Identifier;

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
    pub fn sanitize_value(&self, value: f64) -> f64 {
        value.clamp(self.min_value, self.max_value)
    }
}

pub type AttributeRef = &'static Attribute;

pub struct AttributeRegistry {
    attributes_by_id: Vec<AttributeRef>,
    attributes_by_key: FxHashMap<Identifier, usize>,
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

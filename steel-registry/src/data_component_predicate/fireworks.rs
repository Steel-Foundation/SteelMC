use super::{
    CollectionPredicate, ComponentHasher, DataComponentPredicateCodec, Debug, DowncastType,
    DowncastTypeKey, HashComponent, IntBounds, NbtCompound, NbtNumeric, NbtTag, decode_optional,
    hash_entries, push_hash_entry,
};

/// Fields matched within one firework explosion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireworkPredicate {
    shape: Option<crate::data_components::components::FireworkExplosionShape>,
    has_twinkle: Option<bool>,
    has_trail: Option<bool>,
}

impl FireworkPredicate {
    #[must_use]
    pub const fn new(
        shape: Option<crate::data_components::components::FireworkExplosionShape>,
        has_twinkle: Option<bool>,
        has_trail: Option<bool>,
    ) -> Self {
        Self {
            shape,
            has_twinkle,
            has_trail,
        }
    }

    #[must_use]
    pub const fn shape(
        &self,
    ) -> Option<crate::data_components::components::FireworkExplosionShape> {
        self.shape
    }

    #[must_use]
    pub const fn has_twinkle(&self) -> Option<bool> {
        self.has_twinkle
    }

    #[must_use]
    pub const fn has_trail(&self) -> Option<bool> {
        self.has_trail
    }

    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            shape: decode_optional(compound, "shape", |tag| {
                match tag.string()?.to_owned().try_into_string().ok()?.as_str() {
                    "small_ball" => {
                        Some(crate::data_components::components::FireworkExplosionShape::SmallBall)
                    }
                    "large_ball" => {
                        Some(crate::data_components::components::FireworkExplosionShape::LargeBall)
                    }
                    "star" => {
                        Some(crate::data_components::components::FireworkExplosionShape::Star)
                    }
                    "creeper" => {
                        Some(crate::data_components::components::FireworkExplosionShape::Creeper)
                    }
                    "burst" => {
                        Some(crate::data_components::components::FireworkExplosionShape::Burst)
                    }
                    _ => None,
                }
            })?,
            has_twinkle: decode_optional(compound, "has_twinkle", NbtNumeric::codec_bool)?,
            has_trail: decode_optional(compound, "has_trail", NbtNumeric::codec_bool)?,
        })
    }

    pub(super) fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(shape) = self.shape {
            compound.insert("shape", shape.serialized_name());
        }
        if let Some(has_twinkle) = self.has_twinkle {
            compound.insert("has_twinkle", has_twinkle);
        }
        if let Some(has_trail) = self.has_trail {
            compound.insert("has_trail", has_trail);
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for FireworkPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(shape) = self.shape {
            push_hash_entry(&mut entries, "shape", shape.serialized_name());
        }
        if let Some(has_twinkle) = self.has_twinkle {
            push_hash_entry(&mut entries, "has_twinkle", &has_twinkle);
        }
        if let Some(has_trail) = self.has_trail {
            push_hash_entry(&mut entries, "has_trail", &has_trail);
        }
        hash_entries(hasher, &mut entries);
    }
}

/// Predicate over the `firework_explosion` component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FireworkExplosionPredicate(FireworkPredicate);

impl FireworkExplosionPredicate {
    #[must_use]
    pub const fn new(predicate: FireworkPredicate) -> Self {
        Self(predicate)
    }

    #[must_use]
    pub const fn predicate(&self) -> &FireworkPredicate {
        &self.0
    }
}

impl DataComponentPredicateCodec for FireworkExplosionPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        FireworkPredicate::from_nbt_value(tag).map(Self)
    }

    fn to_nbt_value(&self) -> NbtTag {
        self.0.to_nbt_value()
    }
}

impl HashComponent for FireworkExplosionPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.0.hash_component(hasher);
    }
}

impl_predicate_downcast_type!(
    FireworkExplosionPredicate,
    "steel:data_component_predicate/firework_explosion"
);

/// Predicate over firework explosions and flight duration.
#[derive(Debug, Clone, PartialEq)]
pub struct FireworksPredicate {
    explosions: Option<CollectionPredicate<FireworkPredicate>>,
    flight_duration: IntBounds,
}

impl FireworksPredicate {
    #[must_use]
    pub const fn new(
        explosions: Option<CollectionPredicate<FireworkPredicate>>,
        flight_duration: IntBounds,
    ) -> Self {
        Self {
            explosions,
            flight_duration,
        }
    }

    #[must_use]
    pub const fn explosions(&self) -> Option<&CollectionPredicate<FireworkPredicate>> {
        self.explosions.as_ref()
    }

    #[must_use]
    pub const fn flight_duration(&self) -> IntBounds {
        self.flight_duration
    }
}

impl DataComponentPredicateCodec for FireworksPredicate {
    fn from_nbt_value(tag: &NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self {
            explosions: decode_optional(compound, "explosions", |tag| {
                CollectionPredicate::from_nbt_with(tag, FireworkPredicate::from_nbt_value)
            })?,
            flight_duration: compound
                .get("flight_duration")
                .map_or(Some(IntBounds::ANY), IntBounds::from_owned_nbt)?,
        })
    }

    fn to_nbt_value(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if let Some(explosions) = &self.explosions {
            compound.insert(
                "explosions",
                explosions.to_nbt_with(FireworkPredicate::to_nbt_value),
            );
        }
        if !self.flight_duration.is_any() {
            compound.insert("flight_duration", self.flight_duration.as_nbt_tag());
        }
        NbtTag::Compound(compound)
    }
}

impl HashComponent for FireworksPredicate {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if let Some(explosions) = &self.explosions {
            let mut value_hasher = ComponentHasher::new();
            explosions.hash_with(&mut value_hasher, HashComponent::compute_hash);
            crate::item_predicate::push_prehashed_entry(&mut entries, "explosions", value_hasher);
        }
        if !self.flight_duration.is_any() {
            push_hash_entry(&mut entries, "flight_duration", &self.flight_duration);
        }
        hash_entries(hasher, &mut entries);
    }
}

impl_predicate_downcast_type!(
    FireworksPredicate,
    "steel:data_component_predicate/fireworks"
);

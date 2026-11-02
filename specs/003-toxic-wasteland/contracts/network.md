# Contract: client-visible data

With no tinting fluid and no custom biome, every packet below is byte-identical to the current
output (FR-022).

| Data | Change |
| --- | --- |
| `CRegistryData minecraft:worldgen/biome` | Server biomes in registry order, then the tint variants in `(tint_index, biome id)` order. A variant is its source biome's NBT with `effects.water_color` and `effects.water_fog_color` replaced. |
| `CUpdateTags` biome tags | Each variant is added to every tag that contains its source. |
| `CUpdateTags` fluid tags | Custom fluid ids are never sent; tags list only vanilla fluid ids. |
| Chunk section biome container | Global-palette bits are computed from the client biome count. Tinted cells carry the variant id. |
| `CChunksBiomes` (play 13) | New. At most one per player per tick, listing every chunk whose tinted cells or painted biomes changed that tick. Each buffer holds the projected biome containers of all sections. |
| Block states | The custom liquid block state at offset `o` is sent as `minecraft:water` base state + `o` (or lava). |
| Items | The bucket uses coco's item projection (`minecraft:water_bucket` appearance by default). |

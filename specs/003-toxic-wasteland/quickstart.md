# Quickstart: validating Toxic Wasteland

## Prerequisites

- SteelExtensionMC on branch `003-toxic-wasteland`, with the `extension-example` submodule on the
  same branch.
- A vanilla Minecraft client for the protocol in `vanilla_packets.rs` (no mods, no resource pack).

## Automated checks

```sh
cargo test -p steel-registry client_content      # fluid/biome registration, variant table
cargo test -p steel-core fluid                    # client_fluid projection, fluid_state_to_block parity
cargo test -p steel-core chunk::section           # tint counters, projected biome writes
cargo test -p steel-protocol chunks_biomes        # packet encoding
cargo test -p steel-extension-api --features host --test toxic_wasteland -- --include-ignored
cargo clippy --manifest-path extension-example/toxic-wasteland/Cargo.toml --all-targets
```

The native integration test (`steel-extension-api/tests/toxic_wasteland/`) loads the built
extension and asserts the following:

1. The registries contain `toxic_wasteland:sludge`, `toxic_wasteland:sludge_flowing`, the liquid
   block, the bucket and the biome. The client biome table has one variant per server biome.
2. A sludge source on flat ground spreads to the configured reach and never becomes
   `minecraft:water` on the server.
3. A section containing sludge writes variant ids in the sludge cells, and only there. Draining it
   restores the original ids, and `CChunksBiomes` is queued once.
4. A chunk round-trips through save and load with sludge and painted biomes intact.
5. A pig in sludge gets poison and contact damage. A zombie gets damage but no poison. A creative
   player gets neither.
6. Water beside sludge converts, and grass beside it withers, at the configured rates within ±10%
   over 10,000 random ticks.
7. Sludge meeting a lava source makes coarse dirt, and meeting flowing lava makes cobblestone.
8. The wasteland generator produces the same chunk bytes for the same seed, with lake coverage
   between 2% and 15% over 32×32 chunks.
9. `/toxic paint`, `pour` and `drain` report counts and enforce the volume limit.

## Manual check in a real client

```sh
cd SteelExtensionMC
python3 package-extension.py extension-example/toxic-wasteland --output run/extensions/toxic-wasteland.zip
```

Add the wasteland world to `run/config/worlds.toml`:

```toml
[[domains.minecraft.worlds]]
name = "wasteland"
generator = "toxic_wasteland:wasteland"
```

Then start the server from `run/` (`../target/release/steel`), join, and run `op <you>`.

| Step | Expected |
| --- | --- |
| `/execute in minecraft:wasteland run tp @s 0 120 0` | Olive sky and fog, sickly grass, green lakes. |
| Swim in a lake for a minute | Smooth swimming with no rubber-banding; poison and periodic damage. |
| Back in the overworld: `/toxic pour ~ ~ ~ ~3 ~ ~3` beside a river | The sludge is green, the river stays blue except in the shared 4×4×4 cells, and grass withers over time. |
| Use an empty bucket on a sludge source, then pour it elsewhere | You get a Bucket of Toxic Sludge; pouring places green sludge. |
| `/toxic drain ~-5 ~-2 ~-5 ~5 ~2 ~5` | The sludge is gone and the water returns to its biome color without reconnecting. |
| `//wand`, select an area, `/toxic paint toxic_wasteland:wasteland` | The area turns olive at once. `/toxic paint minecraft:plains` reverts it. |
| Restart the server | The sludge, painted biomes and buckets are all still there. |

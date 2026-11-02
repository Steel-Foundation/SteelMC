---
description: "Task list for Toxic Wasteland (custom fluids, custom biomes, tint projection, extension)"
---

# Tasks: Toxic Wasteland

**Input**: Design documents from `specs/003-toxic-wasteland/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: required. FR-024 and constitution VII require them for the projection, counters,
persistence, gameplay rates and determinism.

**Organization**: by user story. Platform work that several stories share is in Phase 2.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: US1–US6 from spec.md

---

## Phase 1: Setup

- [x] T001 Create the extension crate skeleton `extension-example/toxic-wasteland/` (`Cargo.toml` with a path dependency on `../../steel-extension-api`, `[lib] crate-type = ["cdylib", "rlib"]`, `extension.toml`, `clippy.toml`, the pedantic lint table copied from `extension-example/lucky-block/Cargo.toml`, `.gitignore`, and an empty `src/lib.rs` with `export_extension!` declaring `name: "toxic-wasteland"` and `soft_depends: ["worldedit"]`)
- [x] T002 [P] Add the native test entry `steel-extension-api/tests/toxic_wasteland.rs` with module folder `steel-extension-api/tests/toxic_wasteland/`, register the library in the shared mapping in `steel-extension-api/tests/common/mod.rs`, and mark the tests `#[ignore]` until the library is built, as `lucky_block.rs` does

---

## Phase 2: Foundational (blocking prerequisites)

**Purpose**: the fluid and biome projection layers that every story uses.

- [x] T003 Make `fluid_state_to_block` resolve the block through `Fluid::block` (vanilla `createLegacyBlock`) in `steel-core/src/fluid/state.rs`. Add a parity test asserting identical output for every vanilla water and lava fluid state.
- [x] T004 [P] Add `ClientFluid`, `FluidTint`, `ClientContent::fluids` and `client_fluid(fluid) -> FluidRef` (vanilla fluids map to themselves) in the new `steel-registry/src/client_content/fluid.rs`. Wire it in `steel-registry/src/client_content/mod.rs` and add `FluidState::client_fluid()` in `steel-registry/src/fluid/mod.rs`.
- [x] T005 Implement `Registry::register_client_fluid` in `steel-registry/src/client_content/fluid.rs`:
  - Leak source and flowing `Fluid` statics linked by `source_fluid` and `flowing_fluid`.
  - Build the 16-state liquid block family by copying `minecraft:water`'s layout and shapes, with each state's `StateFluidData` pointing at the custom fluids (level 0 → source; 1–7 → flowing, amount `8 - level`; 8–15 → flowing, falling).
  - Register a `ClientBlock` whose carrier is the water (or lava) state with the same offset.
  - Validate: owner namespace, distinct keys, a tint only with water, `"tick_delay 1..=1200"`, `"drop_off 1..=7"`, `"slope_find_distance 0..=8"`, a finite `"explosion_resistance ≥ 0"`, and colors `"0x000000–0xFFFFFF"`.
- [x] T006 [P] Add registry tests in `steel-registry/src/client_content/tests.rs`:
  - Fluid registration round-trips the key, source and flowing variants.
  - Each liquid state decodes to the right `FluidState` and projects to the water state with the same offset.
  - Duplicate and invalid definitions are rejected.
- [x] T007 Add the `ClientBiomes` variant table (`server_count`, `variants` ordered by `(tint_index, biome id)`, `variant_id`), built at freeze in `steel-registry/src/client_content/biome.rs`. Variant keys use `"<fluid ns>:<fluid path>_tint/<biome ns>/<biome path>"`. Hook it into the freeze sequence in `steel-registry/src/client_content/registration.rs`.
- [x] T008 Send variants in `steel-core/src/server/registry_cache.rs`:
  - Append each variant's NBT (its source's NBT with `water_color` and `water_fog_color` replaced) after the server biomes in the biome `CRegistryData`.
  - Add the variant ids to every biome tag that contains their source.
  - Filter custom fluid ids out of fluid tags.
- [x] T009 [P] Add `CChunksBiomes { chunks: Vec<ChunkBiomeData { pos: ChunkPos, buffer: Vec<u8> }> }` with id `play::C_CHUNKS_BIOMES` in the new `steel-protocol/src/packets/game/chunk/biomes.rs`, exported from `chunk/mod.rs`. Add an encoding test against a hand-built buffer.
- [x] T010 Make `biome_global_bits()` use the client biome count, and add a projected write helper `BiomePalette::write_projected(&self, writer, project: impl Fn(cell, u16) -> u16)` in `steel-core/src/chunk/paletted_container.rs`. Add tests: unchanged bytes without variants; correct ids in global mode when variants exist.
- [x] T011 Add `tint_cells` counters to `ChunkSection` in `steel-core/src/chunk/section.rs`:
  - The sparse `(tint_index, [u16; 64])` rows are allocated on first tinting state.
  - They are updated in the same path as `apply_count_change` and rebuilt in `recalculate_counts`.
  - `dominant_tint(cell)` returns the most blocks, with ties going to the lowest tint index.
  - Setting a block returns whether any cell's dominant tint changed.
- [x] T012 Make `ChunkSection::write` in `steel-core/src/chunk/section.rs` use `write_projected` when any cell is tinted, and the plain write otherwise. Add tests: a section without sludge writes identical bytes; sludge cells carry variant ids; draining restores the original ids; a debug check that counters equal a full recount.
- [x] T013 Add `changed_biome_sections` with `mark_biomes_changed(section)` and `take_changed_biomes()` to `ChunkHolder` in `steel-core/src/chunk/chunk_holder.rs`, and call it from the world's block-set path when T011 reports a tint change (`steel-core/src/world/mod.rs`).
- [x] T014 Extend the broadcast loop in `steel-core/src/chunk/chunk_map/mod.rs` to drain `take_changed_biomes()`, build one `ChunkBiomeData` per chunk (projected containers of every section), and send a single `CChunksBiomes` per tracking player per tick, after the block updates. Add a test that two changes in one tick produce one packet.
- [x] T015 Route entity movement through `client_fluid()` in `steel-core/src/entity/fluid_contact.rs` (heights, currents, eye-in-fluid), `steel-core/src/entity/living_entity.rs` (`jump_in_liquid`) and `steel-core/src/entity/entity/mod.rs` (`block_fluid_is_water`). Doc-comment the deviation from tag checks: movement follows what the client predicts. Add a test that a custom water-displayed fluid sets `water_height`.
- [x] T016 Add the `react_with_neighbors(world, pos) -> bool` hook to `FluidBehavior` in `steel-core/src/fluid/fluid_behavior.rs`, with a default that keeps vanilla behavior. Move lava's obsidian, cobblestone and basalt logic from `steel-core/src/behavior/blocks/fluid/liquid_block.rs` into `LavaFluid` (`steel-core/src/fluid/fluids/lava.rs`), and have `LiquidBlock::should_spread_liquid` call the hook. The existing lava tests must pass unchanged.
- [x] T017 Add `ExtensionFluid` (`FluidBehavior` + `FlowingFluid`) to the new `steel-core/src/fluid/fluids/extension.rs`. It uses definition data (tick delay, drop-off, slope, source conversion, explosion resistance) and optional hook callbacks, and it applies the client-kind base effect (water: `Extinguish`) before calling the hook. Register its behaviors and the `LiquidBlock` for every client fluid during behavior init (`steel-core/src/behavior/mod.rs`).

**Checkpoint**: the core can host a projected, tinted custom fluid. The vanilla test suites still pass.

Implementation notes for Phase 2:
- T008: Steel does not send `worldgen/biome` tags yet, so variant tag membership is a `// TODO:` in `registry_cache.rs`.
- T010: there is no separate `write_projected` helper; `TintCells::project` builds the client palette (`chunk/section_tint.rs`).
- T013: the broadcast loop reads the section flags of chunks that already have block changes, so the world set path needs no extra call. Painted biomes use `ChunkMap::client_biomes_changed`.
- T012, T014, T015: tests that need a custom fluid in the global registry run in the native integration test (T022), because unit tests share one vanilla-only process registry. `TintCells` has its own unit tests.
- T017: fluid hooks use `FluidBehaviorRegistry::on_registration`, which is the same pattern as block behaviors. The extension API therefore gets its own `Hooks::register_fluid_behaviors` instead of `Registry::set_fluid_behavior`. `LiquidBlock::pickup_block` now returns the fluid's own bucket (part of T034).

---

## Phase 3: User Story 1 - Sludge is a real fluid that looks green and behaves like a liquid (P1) 🎯 MVP

**Goal**: an extension registers a tinted fluid that spreads, swims like water, persists, and
looks green everywhere.

**Independent test**: quickstart integration checks 1–4, plus the manual swim and pour steps.

- [x] T018 [P] [US1] Add the stabby `FluidDefinition` (fields and defaults exactly as in data-model.md), `ClientFluidKind`, `FluidTint`, `RegisteredFluid` and `ContentRegistry::register_fluid` to the new `steel-extension-api/src/content/fluid.rs`, re-exported from `content/mod.rs` and bound in `content/bindings.rs`.
- [x] T019 [US1] Add host construction and validation to the new `steel-extension-api/src/host/content/fluid.rs`, calling `register_client_fluid`. Add unit tests for every rejected field in `host/content/tests/fluid.rs`.
- [x] T020 [P] [US1] Add the stabby `DynFluidBehavior` trait (`entity_inside`, `random_tick`, `before_destroying_block -> bool`, `react_with_neighbors -> bool`) and `Registry::set_fluid_behavior` to the new `steel-extension-api/src/fluid_behavior.rs`, and add the host bridge that installs the callbacks into `ExtensionFluid`. Only the registering extension may set hooks for its fluid.
- [x] T021 [US1] Add `toxic_wasteland:sludge` (flowing `toxic_wasteland:sludge_flowing`, `client = Water`, a tint from the configured colors, `tick_delay` 15, `drop_off` 2, no source conversion, `randomly_ticking`) to the new `extension-example/toxic-wasteland/src/content.rs`. Load the config in the new `src/config.rs` (serde, hex colors, validation that names the failing field).
- [x] T022 [US1] Add native integration tests in `steel-extension-api/tests/toxic_wasteland/fluid.rs`:
  - The registry contains the fluid pair, block and variants.
  - Spreading on flat ground reaches the configured distance and never yields `minecraft:water`.
  - A chunk containing sludge round-trips through save and load.
  - Projected section bytes carry the variant only in sludge cells, and draining queues exactly one `CChunksBiomes`.

**Checkpoint**: MVP. Green, swimmable, persistent sludge exists.

---

## Phase 4: User Story 2 - A toxic biome that clients see (P1)

**Goal**: extensions register biomes and change them at runtime with live client updates.

**Independent test**: register the biome, paint a region, observe it, and restart.

- [x] T023 [P] [US2] Add the stabby `BiomeDefinition` and `BiomeColors` (fields as in data-model.md: required sky, fog, water and water-fog colors; optional grass, foliage, dry-foliage, modifier, particle and music) and `ContentRegistry::register_biome` to the new `steel-extension-api/src/content/biome.rs`.
- [x] T024 [US2] Add host construction to the new `steel-extension-api/src/host/content/biome.rs`, building `steel_registry::biome::Biome` (empty features, carvers and spawners, with `// TODO:` for decorators). Validate `"colors 0x000000–0xFFFFFF"`, finite temperature and downfall, and that sound and particle keys resolve. Register it before freeze, with unit tests.
- [x] T025 [US2] Add core `World::fill_biome(from, to, biome) -> Result<u64, FillBiomeError>` (vanilla `/fillbiome` quart semantics; loaded chunks only; reports unloaded chunk counts) and `World::biome_at` to the new `steel-core/src/world/biomes.rs`. They mark chunks unsaved and sections biome-changed. Add tests for counts, persistence and the unloaded-chunk error.
- [x] T026 [US2] Expose `World::biome_at` and `World::set_biomes` in `steel-extension-api/src/worlds.rs`, with host glue in `steel-extension-api/src/host/`.
- [x] T027 [US2] Register `toxic_wasteland:wasteland` from the config colors (`has_precipitation = false`, `temperature 1.2`, `downfall 0.0`, ash particles) in `extension-example/toxic-wasteland/src/content.rs`.
- [x] T028 [US2] Add integration tests in `steel-extension-api/tests/toxic_wasteland/biome.rs`: the client registry contains the biome NBT with the configured colors; `set_biomes` changes the cells and queues one `CChunksBiomes`; painted biomes survive a save and load; a missing-key load fails with the existing error.

---

## Phase 5: User Story 3 - Sludge is dangerous (P2)

**Goal**: poison and damage, plant destruction, ground withering, water corruption, and the lava
reaction.

**Independent test**: the gameplay rate tests (quickstart checks 5–7).

- [x] T029 [P] [US3] Add a versioned `entity/v2` living-entity API to the new `steel-extension-api/src/block_behavior/entity_v2.rs`: `is_living`, `is_creative_or_spectator`, `position`, `hurt(damage_type, amount) -> ApiResult<bool>` and `add_effect(effect) -> ApiResult<bool>`. Host glue uses `LivingEntity::hurt` and `add_effect`, so vanilla immunity and invulnerability apply. Unknown keys return `Err`.
- [x] T030 [US3] Add the hazards in the new `extension-example/toxic-wasteland/src/hazards.rs`:
  - `entity_inside`: poison (respecting immunity) plus contact damage via `minecraft:magic`, paced by `damage_interval_ticks` per entity UUID, with pruning. Creative and spectator players are exempt.
  - `random_tick`: water corruption (same level) and grass, dirt, podzol and mycelium withering to coarse dirt at the configured chances. Each can be disabled with a chance of 0.
- [x] T031 [US3] Implement `react_with_neighbors` in `extension-example/toxic-wasteland/src/sludge.rs`: an adjacent lava source becomes coarse dirt, flowing lava becomes cobblestone, with `minecraft:block.lava.extinguish` and the fizz level event. Use default drops in `before_destroying_block`.
- [x] T032 [US3] Add integration tests in `steel-extension-api/tests/toxic_wasteland/hazards.rs`: a pig gets poison and damage; a zombie gets damage but no poison; a creative player is unaffected; crops are destroyed with drops; corruption and withering hit ±10% of their configured rates over 10,000 random ticks; both lava reactions work.

---

## Phase 6: User Story 4 - A sludge bucket (P2)

**Goal**: pick up and pour sludge like water.

**Independent test**: survival and creative pick-up and pour, nether evaporation, and persistence.

- [x] T033 [US4] Generalize `steel-core/src/behavior/items/bucket.rs` from `fluid_block == WATER` checks to the bucket's fluid. Water-like rules (evaporation, sounds, cauldrons) use `client_fluid().is_water()`; waterlogging stays real-water-only (documented deviation). The vanilla bucket tests must pass unchanged.
- [x] T034 [US4] When a `FluidDefinition` has a `bucket`, register its item with the core bucket behavior for that fluid and set `Fluid::bucket_item`, so `LiquidBlock::pickup_block` returns it (`steel-extension-api/src/host/content/fluid.rs`, `steel-core/src/behavior/items/mod.rs`).
- [x] T035 [US4] Declare the "Bucket of Toxic Sludge" (`minecraft:water_bucket` appearance, stack size 1) in `extension-example/toxic-wasteland/src/content.rs`.
- [x] T036 [US4] Add integration tests in `steel-extension-api/tests/toxic_wasteland/bucket.rs`: pick-up from a source only; pouring in survival empties the bucket and in creative keeps it; nether evaporation; a waterloggable target does not get waterlogged; the bucket persists in an inventory.

---

## Phase 7: User Story 5 - The wasteland world (P3)

**Goal**: a seeded barren world in the toxic biome with sludge lakes.

**Independent test**: generator determinism and lake coverage (quickstart check 8).

- [x] T037 [US5] Add the `toxic_wasteland:wasteland` terrain generator (overworld dimension type, the toxic biome, sea level 63) to the new `extension-example/toxic-wasteland/src/generator.rs`:
  - The heightmap is three-octave seeded value noise using `base_height` and `relief`.
  - Chunk-local sludge basins appear with probability `lake_chance`, from a hash of the seed and chunk position. They never write outside the chunk.
  - Coarse dirt and gravel cover the surface over stone.
- [x] T038 [US5] Add integration tests in `steel-extension-api/tests/toxic_wasteland/generator.rs`: the same seed gives identical chunk bytes; every biome cell is toxic; lake coverage stays within 2–15% over 32×32 chunks.

---

## Phase 8: User Story 6 - Admin paint and pour tools (P3)

**Goal**: the `/toxic paint | pour | drain` commands from [contracts/commands.md](contracts/commands.md).

**Independent test**: each command on a known region.

- [x] T039 [US6] Add the `/toxic` command tree (operator level 2) to the new `extension-example/toxic-wasteland/src/commands.rs`:
  - The region comes from explicit corners or the `worldedit:selection/v1` service.
  - `max_volume` is enforced.
  - `paint` uses `set_biomes`; `pour` and `drain` use the bulk `World::fill` with replace filters, and `pour` schedules border fluid ticks.
  - The feedback and error texts match the contract.
- [x] T040 [US6] Add integration tests in `steel-extension-api/tests/toxic_wasteland/commands.rs`: paint, pour and drain counts; the volume-limit error; the missing-selection error; painting `minecraft:plains` back.

---

### Implementation notes for Phases 3–8

- T002/T022–T040: the native checks share one test, `steel-extension-api/tests/toxic_wasteland.rs`, because registries initialize once per process. It covers registration, liquid and fluid projection, spreading reach with no water conversion, tint projection and refresh flags, hazards and their pacing, rates within ±10% over 10,000 random ticks, both lava reactions, the bucket pickup, and persistence of sludge and painted biomes.
  - The test calls fluid ticks and hooks directly, because the scheduler internals are crate-private.
  - The zombie (undead) case is not tested: Steel implements no undead mob yet. Poison immunity comes from vanilla `canBeAffected` inside `add_mob_effect`.
  - Generator determinism, coverage and containment, and command regions are unit tests in the extension crate, since they do not need a world.
- T020: one-time hooks per fluid instead of owner checks. A second `set_fluid_behavior` for the same fluid is an error.
- T039: `pour` replaces air and replaceable blocks, and `drain` replaces the 16 sludge states, through a new `World::replace_blocks` binding (`fill::replace_api`) backed by `World::replace_blocks` in core. The existing `fill` binding is unchanged.
- New bindings added along the way: `World::game_time` for damage pacing, and `World::schedule_fluid_tick` so poured sludge spreads.
- The default `world.lake_chance` is 0.3 instead of 0.12, so lakes cover 2–15% of the surface (SC-007).

## Phase 9: Polish & cross-cutting

- [x] T041 [P] Add the "Custom fluids", "Custom biomes" and "Tint projection" sections to `doc/extensions/custom-content.md`, and add the reference entries to `doc/extensions/api.md`.
- [x] T042 [P] Write `extension-example/toxic-wasteland/README.md` (install, config table, commands, world setup, limitations: shared-cell tint, no waterlogging, F3 variant names, no dispenser) and list the crate in `extension-example/README.MD`.
- [x] T043 Add a benchmark-style test asserting that painting 256×256×384 takes under 2 s in release (`steel-core/src/world/biomes.rs`, `#[ignore]` by default), and record the measured numbers in the README.
- [x] T044 Run the gates: `cargo fmt`, `cargo clippy --workspace --all-targets --features steel-extension-api/host`, the core, registry, protocol and API test suites, every native integration test with all examples built, and clippy on the extension crate.
- [x] T045 Run the quickstart manually on a release server with all extensions and confirm that a server without the extension sends byte-identical chunk and registry data (a test in `steel-extension-api/tests/toxic_wasteland/parity.rs`).

---

### Polish results

- T043: painting 256×256×384 (393,216 cells) takes 2.3 ms in release, against the 2 s target.
- T045 (live run): a release server loaded Vault, Essentials, WorldEdit, WorldGuard and toxic-wasteland, generated the wasteland world (spawn at y=81), and stopped cleanly with no warnings.
- T045 (parity): Steel's existing chunk and registry encoding tests pass unchanged. Without a tinting fluid, sections write their biomes unchanged and the client biome count equals the server count. A dedicated byte-comparison test was not added.
- Not verified: a session with a real client (colors in game, swimming without corrections). The quickstart's manual steps cover it.

## Dependencies & execution order

- **Setup** (T001–T002) → **Foundational** (T003–T017) → the stories.
- Inside Phase 2:
  - T003 is independent.
  - T004 → T005 → T006.
  - T007 → T008.
  - T009 is independent.
  - T010 → T011 → T012 → T013 → T014.
  - T015 needs T004; T016 is independent; T017 needs T005 and T016.
- **US1** needs all of Phase 2. **US2** needs T007–T014 (projection and broadcast) and is
  independent of US1's API.
- **US3** and **US4** need US1. **US5** needs US1 and US2. **US6** needs US1 and US2.
- **Polish** comes after all stories.

### Parallel opportunities

- Phase 2: T003, T004, T007, T009 and T016 touch different files.
- US1: T018 and T020 are parallel; the tests in T022 come after T021.
- US2: T023 and T025 are parallel.
- US3: T029 is parallel to T031.
- Polish: T041 and T042 are parallel.

## Implementation strategy

1. **MVP**: Phases 1–3, giving green, swimmable, persistent sludge placed by the API.
2. Then US2 (biome and paint), US3 (hazards) and US4 (bucket), which make the content complete.
3. Then US5 (world) and US6 (commands), the showcase and the admin tools.
4. Commit locally at each phase checkpoint, keeping the extension-example submodule commit before
   the main repo commit.

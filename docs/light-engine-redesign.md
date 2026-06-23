# Steel Light Engine Redesign Spec

This document captures the behavior and integration contracts needed for the
`worldgen/light2` branch. It was originally written from the `worldgen/light`
branch and should treat that old branch as an executable reference, not as the
current branch state.

The goal is not to preserve the old ScalableLux-shaped ownership model. The
goal is to preserve vanilla/Steel behavior while making the storage and edit
boundaries feel designed for Rust.

## Reference Scope

Reference priority:

1. Local vanilla source under `minecraft-src/minecraft/`, generated for the
   workspace target version in `Cargo.toml` (`0.11.0+mc26.2`).
2. Old Steel light checkout at `/home/alve/Documents/minecraft/SteelMC-oldlight`,
   branch `worldgen/light`, for Steel-specific ScalableLux integration,
   persistence, and packet behavior.
3. ScalableLux checkout at `/home/alve/Documents/minecraft/ScalableLux`, for the
   Starlight-derived cache, queue, and nibble-state design that old Steel ported.
4. The current `worldgen/light2` branch, which is the implementation target.

At this branch point, `worldgen/light2` does not yet contain the old light
module. `steel-core/src/worldgen/stages/light.rs` is a no-op, and
`LevelChunk::extract_light_data` still emits all-`0xff` sky and block light for
every padded light section. The rest of this document describes the target and
old-reference behavior, not the current implementation.

Old Steel files to treat as the Steel reference implementation:

- `steel-core/src/chunk/light/mod.rs`: shared constants, opacity/occlusion helpers, public exports.
- `steel-core/src/chunk/light/data_layer.rs`: vanilla-compatible packed section data.
- `steel-core/src/chunk/light/section_storage.rs`: padded light-section range.
- `steel-core/src/chunk/light/nibble.rs`: old chunk-owned light section state.
- `steel-core/src/chunk/light/cache.rs`: old-reference flat cache/window indexing.
- `steel-core/src/chunk/light/queue.rs`: vanilla and ScalableLux queue metadata.
- `steel-core/src/chunk/light/workset.rs`: scoped chunk/section/light access.
- `steel-core/src/chunk/light/propagation.rs`: block-light propagation.
- `steel-core/src/chunk/light/sky_propagation.rs`: sky-light propagation.
- `steel-core/src/chunk/light/sky_sources.rs`: per-column skylight source cache.
- `steel-core/src/chunk/light/packet.rs`: client light update conversion.
- `steel-core/src/worldgen/stages/light.rs`: worldgen/load-stage integration.
- `steel-core/src/chunk_saver/storage.rs` and `format.rs`: persistent light conversion.
- `steel-core/src/chunk/chunk_map.rs`, `proto_chunk.rs`, `level_chunk.rs`, and `world.rs`: dynamic block-change integration.

Vanilla classes to keep nearby while validating behavior:

- `LevelLightEngine`, `LightEngine`, `LayerLightSectionStorage`
- `BlockLightEngine`, `SkyLightEngine`
- `BlockLightSectionStorage`, `SkyLightSectionStorage`
- `ChunkSkyLightSources`
- `ClientboundLightUpdatePacketData`
- `LightChunk`, `LightChunkGetter`

ScalableLux files to keep nearby when validating the old-reference algorithm:

- `SWMRNibbleArray`
- `StarLightEngine`
- `BlockStarLightEngine`
- `SkyStarLightEngine`
- `StarLightInterface`

## Design Goal

The new implementation should separate three concepts that are currently mixed
inside `LightNibbleArray`:

1. Chunk-owned visible light state.
2. Scoped mutable light edits during one propagation operation.
3. Algorithm-specific cache/queue/indexing strategy.

The public chunk storage should not expose ScalableLux terms such as "nibble
cache", "SWMR", or "updating state" unless those terms describe actual Steel
semantics. A scoped edit object may still use flat arrays and packed queues
internally if that remains the best algorithm.

## Non-Goals

- Do not change vanilla light propagation behavior while redesigning ownership.
- Do not collapse missing light and zero light into one state.
- Do not remove edge validation, hidden/internal light sections, sky extrusion,
  or section emptiness handling without a separate parity-driven decision.
- Do not add runtime datapack behavior or plugin APIs as part of this rewrite.
- Do not introduce async or multithreading inside lighting unless there is a
  concrete scheduling reason.

## External Contracts

### Light Section Range

- The light section range is the real chunk section range padded by one section
  below and one section above.
- Real section emptiness maps are indexed only by real chunk sections, not by
  the padded light sections.
- Packet section indices are ascending indices into the padded light section
  range.

### Packed Light Data

- One light section stores `16 * 16 * 16` 4-bit values in `2048` bytes.
- Local block index is `x | (z << 4) | (y << 8)`.
- Values are masked to `0..=15` on write.
- Packet bytes must stay in vanilla low-nibble-first order.

### Section Data And Presence Semantics

The new design can rename these states, but it must preserve the four meanings:

- Missing/null: no section light data exists. Block-light reads behave as zero.
  Sky-light reads may search upward or return full sky depending on chunk height
  and known non-empty sections.
- Present zero/uninitialized: the section exists and all values are zero without
  requiring backing bytes. Packets use the empty light mask. Persistence records
  an uninitialized section.
- Visible data/initialized: the section exists and has visible light data.
  Packets include the bytes unless the data is all-zero.
- Hidden/internal: the section has light data and can participate in internal
  light reads, but is omitted from vanilla packet conversion. Non-zero hidden
  data is currently persisted in Steel's format as hidden; all-zero hidden data
  canonicalizes to omitted.

Vanilla `DataLayer` also represents homogeneous non-zero data without backing
bytes, especially full sky sections. The Rust storage should model data
representation separately from section presence:

This is a good candidate Rust storage shape:

```rust
enum LightSectionData {
    Homogeneous(u8),
    Packed(Box<[u8; DATA_LAYER_SIZE]>),
}

enum LightSection {
    Missing,
    Visible(LightSectionData),
    Internal(LightSectionData),
}
```

Names are not final. The important properties are:

- Missing, externally visible, and internal-only are type-level states.
- `Visible(Homogeneous(0))` is the old uninitialized/empty-mask state.
- `Visible(Homogeneous(15))` is visible full-sky data and must serialize to an
  update payload, not the empty mask.
- `Packed` data keeps vanilla low-nibble-first byte order.
- Values are masked to `0..=15` when written.

### Packet Conversion

Match vanilla `ClientboundLightUpdatePacketData.prepareSectionData`:

- Missing/internal sections are omitted.
- Visible homogeneous zero sections set the empty mask.
- Visible non-zero sections set the update mask and append bytes. Homogeneous
  non-zero sections expand to vanilla packed bytes for the packet payload.
- Updates are emitted in ascending light-section-index order.
- Sky data is omitted entirely when the dimension has no skylight.

### Persistence

- Null/missing sections are omitted.
- Visible homogeneous zero sections persist as uninitialized.
- Visible non-zero sections persist packed bytes in the current old-light Steel
  format.
- Internal/hidden non-zero sections persist as internal/hidden Steel light data.
- All-zero visible packed sections canonicalize to present zero.
- All-zero internal/hidden sections canonicalize to omitted.
- Loading a chunk below `ChunkStatus::Light` returns fresh empty light storage.
- Loaded sky light normalizes null sections below loaded sky data into explicit
  zero sections.

### World Light Reads

- Block light returns the visible value when a present section exists, otherwise zero.
- Sky light returns the visible value when a present section exists.
- If sky data is missing at the queried section, sky light searches upward for
  the next present section in the column and samples its bottom row.
- If sky data is missing above the highest non-empty section, sky light returns
  full light.
- Dimensions without skylight return zero sky light at the world API boundary.

## Integration Contracts

### Chunk Construction

- `ProtoChunk` and `LevelChunk` own both block and sky light storage.
- They also own `ChunkSkyLightSources`.
- `initialize_light_sources` must recalculate section counts, refresh light
  emptiness maps, and fill sky-light source columns from section contents.

### Generation Pipeline

- `InitializeLight` runs after `Features`.
- `Light` depends on `InitializeLight` at radius 1.
- The loading pyramid uses a separate `load_light` task for loaded chunks.
- Fresh light generation:
  - Requires center chunk at `InitializeLight`.
  - Reads center and initialized neighbors.
  - Writes center and already-lit neighbors only.
  - Old-light runs sky first when the dimension has skylight, then block light.
  - Publishes changed light sections to the world/chunk update layer.
- Loaded light:
  - Requires center chunk at `Light`.
  - Force-synchronizes loaded light sections.
  - Then validates sky/block chunk edges.

### Dynamic Block Changes

- `set_block_state` must detect section empty/non-empty transitions.
- Light is queued when light properties changed or section emptiness changed.
- Light properties changed if dampening, emission, or either state's
  shape-for-light-occlusion flag differs.
- Sky-light sources update when light properties change.
- `ChunkMap` drains pending light work before broadcasting changed chunks.
- Dynamic propagation uses a full-radius workset. Old-light/ScalableLux run sky
  changes before block changes.

### Layer Order

This is a verified fork, not a point to infer from memory:

- Vanilla `LevelLightEngine.runLightUpdates` executes block updates before sky
  updates.
- Old Steel `worldgen/light` and ScalableLux execute sky before block for fresh
  chunk lighting and queued dynamic changes.

The rewrite must choose one order deliberately. If old-light is the executable
reference, keep sky-before-block and cover it with parity tests. If the goal is
a direct vanilla light-engine port instead, switch to block-before-sky and
revalidate chunk-generation and dynamic-update output.

### Locking and Worksets

- Worksets must hold chunk holders long enough to keep chunks alive during a
  lighting operation.
- Locks should be scoped and acquired in deterministic slot order.
- Avoid storing long-lived references into chunk internals.
- Section reads and light writes are separate capabilities. Some chunks are
  readable but not writable during generation.

## Algorithm Behavior To Preserve

### Shared Propagation Rules

- Maximum light is 15.
- Propagation opacity clamps block light dampening to at least 1.
- Shape occlusion must use vanilla light occlusion semantics:
  - If both shapes are empty, use simple opacity.
  - Otherwise merged face occlusion blocks light with opacity 16.
- Direction iteration order is currently pinned to ScalableLux order:
  `+X, -X, +Z, -Z, +Y, -Y`.
- Vanilla queue-entry direction bits use `Direction.ordinal()` order
  `Down, Up, North, South, West, East`.
- Increase and decrease queues are separate FIFO queues.
- Decrease processing can enqueue increases, then increase processing runs.
- Edge checks collect delayed local indices and then run regular block checks.

Queue representation has two distinct contracts:

- Vanilla `LightEngine.QueueEntry` bit layout is behaviorally relevant when
  matching vanilla queue metadata.
- ScalableLux packed queue-position encoding is an internal optimization for the
  old-reference propagator. Keep it inside `LightEdit`/propagation internals if
  retained; do not expose it from chunk-owned storage or packet/persistence APIs.

### Cache Window

The old-reference implementation uses:

- 5x5 chunk window for setup.
- Inner 3x3 window for section/nibble cache population.
- Padded light section range plus one extra vertical cache buffer above and below.
- Packed queue positions limited to a 64x64 horizontal window around the center.

These details can be internal implementation choices in the new branch, but
changing iteration windows or order needs parity tests because it can affect
edge behavior and deterministic updates.

### Block Light

Fresh chunk lighting:

- Reset center block-light sections to missing/null.
- Synchronize sections around non-empty block sections.
- Existing initialized all-empty block-light sections can become hidden/internal;
  fresh missing all-empty sections may remain missing.
- Seed block light sources from the center chunk in deterministic local-index order.
- With edge checks required: propagate increases, validate edges, then resolve decreases/increases.
- With edge checks skipped: pull initialized neighbor edge levels inward, then propagate increases.

Dynamic block changes:

- Apply section empty transitions first.
- Re-sync block-light sections for changed chunks.
- For each changed block:
  - Read current light.
  - Set emitted level.
  - Enqueue increase if emission is non-zero.
  - Enqueue decrease for the old current level.
- Resolve decreases, then increases.

### Sky Light

Fresh chunk lighting:

- Reset center sky-light sections to missing/null.
- Rewrite null sky sections out of the temporary writable cache before skylight work.
- Initialize light sections around non-empty sections.
- Sections above the highest non-empty section fill with full sky.
- Sections below may be initialized by extruding the bottom row from the first
  non-null section above.
- Full empty-section edge propagation can seed full-light edge entries.
- Required edge checks run after initial sky-source propagation.
- Skipped edge checks pull initialized neighbor levels inward.
- After required fresh lighting, empty sections are deinitialized/lazily
  initialized again to match reference behavior.

Dynamic sky changes:

- Apply section empty transitions first.
- Deinitialize/lazily initialize sky sections for changed chunks.
- Initialize sections around changed blocks in the center chunk.
- For changed columns, attempt delayed full-sky propagation downward and remove
  sky sources below the new barrier.
- Apply delayed writes/deletes, then run regular block checks and propagation.

Loaded sky:

- Force-load synchronizes empty-section state without resetting existing light.
- Edge validation rewrites null sky sections out of the temp cache, checks null
  sections top-down, and validates horizontal edges.

### Sky-Light Sources

- Each chunk tracks the lowest skylight source edge per X/Z column.
- Empty chunks extend sources below the world and expose negative infinity as
  the public "full sky from above" sentinel.
- A source edge exists where the top/bottom block pair occludes skylight, or
  where the bottom block has non-zero light dampening.
- Column updates ignore block changes below the current source edge unless the
  checked edge is the current source edge.

## Rust-Native Architecture Target

Suggested ownership boundaries:

- `ChunkLight`: chunk-owned visible block/sky light and section emptiness.
- `LightLayerStorage`: one layer's visible section array and conversion helpers.
- `LightSection`: typed section presence/data state.
- `LightEdit`: scoped mutable edit state for one layer and one workset.
- `LightWorkset`: holder pinning plus scoped chunk/section/light admission.
- `LightReadView`: immutable section/block-state access for propagation.
- `LightWriteView`: mutation API over a `LightEdit`, not direct chunk storage.
- `LightUpdateSet`: changed sections to publish after commit.

Expected flow:

1. Build a workset for one center chunk.
2. Build a section read view and layer edit view.
3. Run block or sky propagation against the views.
4. Commit the edit view into chunk-owned visible light.
5. Return changed sections for packet/world notifications.

The chunk-owned storage should not need an always-present visible/updating pair.
Copy-on-write or cloned data should be local to `LightEdit` unless profiling
shows a reason to keep persistent shared buffers.

Edit/commit invariants:

- Workset setup decides readable and writable chunks before propagation starts.
- Propagation writes only through `LightEdit`/`LightWriteView`.
- Chunk-owned `ChunkLight` is not mutated directly while propagation is running.
- Transient sections created for sky extrusion or null-section edge checks can
  exist in the edit view without committing to chunk storage.
- Commit canonicalizes section data, publishes changed sections in stable order,
  and drops any transient-only sections.
- Packet and persistence conversion read only committed chunk-owned state.

Recommended implementation sequence:

1. Reintroduce `DataLayer`, light-section range, packet conversion, and
   chunk-owned storage with unit tests.
2. Reintroduce persistence conversion and loaded-sky normalization.
3. Add `ChunkSkyLightSources` and `initialize_light_sources` to proto/full chunks.
4. Add light worksets and fresh/loaded worldgen lighting.
5. Add queued dynamic block-change lighting and packet broadcast integration.
6. Optimize the internal propagation queue/cache only after parity tests are in
   place.

## Subtle Improvements Allowed

The rewrite may improve structure if behavior remains pinned:

- Rename `Null`, `Uninitialized`, `Initialized`, `Hidden` to domain terms.
- Move visible/updating state out of chunk-owned sections into scoped edits.
- Represent homogeneous non-zero `DataLayer` sections without requiring packed
  bytes until packet/persistence conversion needs them.
- Hide packed ScalableLux queue bits behind typed constructors.
- Replace temporary "removed null nibble" behavior with a typed transient edit
  state, as long as transient sections still do not commit to chunk storage.
- Group block-light and sky-light shared edge/queue helpers if tests stay clear.
- Keep comments about vanilla/ScalableLux only where they explain a preserved
  behavioral constraint.

## Required Parity Tests

Before replacing the current branch, keep or add tests for:

- Light section range padding for normal and overworld heights.
- Packet masks for missing, homogeneous zero, homogeneous non-zero, packed data,
  hidden/internal, filtered sections, and no-skylight dimensions.
- Initial chunk packets must come from chunk-owned light state; the current
  all-`0xff` placeholder path should fail once real light tests are enabled.
- Save/load roundtrip for zero, data, hidden/internal, and loaded sky normalization.
- World light reads for block light, sky light upward search, and full sky above highest non-empty.
- `ChunkSkyLightSources` fill/update behavior.
- Dynamic block changes for emission, opacity, section empty transitions, and sky-source changes.
- Fresh worldgen light for sky and block light.
- Loaded light preserving interior data while validating edges.
- Block edge checks pulling missing neighbor light and removing stale center light.
- Sky edge checks pulling neighbor light under ceilings and skipping null center extrusion.
- Direction order, vanilla queue flags, ScalableLux packed queue entries, and FIFO
  behavior if either queue representation remains.
- Layer-order parity for the chosen reference order.

Recommended high-level parity harness:

- Generate a small fixed set of chunks on the reference branch.
- Record chunk light packets and saved light data hashes.
- Run the same seeds/dimensions on the redesigned branch.
- Compare both section update sets and final visible packet data.

## Open Decisions

- Hidden/internal persistence: keep old-light behavior for now. Persist non-zero
  internal sections and omit all-zero internal sections until parity data proves
  hidden data can become purely transient.
- Chunk-owned storage shape: prefer a dense array over the padded light-section
  range for the first Rust-native implementation. It matches old-light chunk
  ownership and keeps packet index conversion simple. Revisit sparse storage
  only if direct vanilla `DataLayerStorageMap` ownership becomes the chosen
  architecture.
- Queue encoding: keep ScalableLux packed queue encoding only inside
  propagation/edit internals while old-light parity is the reference. Do not
  make it part of chunk storage. It can be replaced after parity tests prove
  order and output are unchanged.
- Shared propagation context: keep block and sky contexts separate until the
  algorithms are stable. Extract shared helpers only where tests show the
  duplicated code is mechanically identical.
- Naming: prefer `Missing`, `Visible`, `Internal`, `Homogeneous`, and `Packed`
  over `Null`, `Uninitialized`, `Initialized`, `Hidden`, `Nibble`, or `SWMR` in
  public Steel APIs.
- Layer order: choose old-light/ScalableLux sky-before-block or vanilla
  block-before-sky explicitly before implementing dynamic propagation.

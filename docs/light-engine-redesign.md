# Steel Light Engine Redesign Spec Draft

This document captures the behavior and integration contracts from the
`worldgen/light` branch so a new Rust-native lighting branch can use the current
implementation as an executable reference.

The goal is not to preserve the current ScalableLux-shaped ownership model. The
goal is to preserve vanilla/Steel behavior while making the storage and edit
boundaries feel designed for Rust.

## Reference Scope

Current Steel files to treat as the reference implementation:

- `steel-core/src/chunk/light/mod.rs`: shared constants, opacity/occlusion helpers, public exports.
- `steel-core/src/chunk/light/data_layer.rs`: vanilla-compatible packed section data.
- `steel-core/src/chunk/light/nibble.rs`: current chunk-owned light section state.
- `steel-core/src/chunk/light/cache.rs`: current flat cache/window indexing.
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

### Section Presence Semantics

The new design can rename these states, but it must preserve the four meanings:

- Missing/null: no section light data exists. Block-light reads behave as zero.
  Sky-light reads may search upward or return full sky depending on chunk height
  and known non-empty sections.
- Zero/uninitialized: the section exists and all values are zero without
  requiring backing bytes. Packets use the empty light mask. Persistence records
  an uninitialized section.
- Data/initialized: the section exists and owns packed bytes. Packets include
  the bytes unless the data is hidden/internal.
- Hidden/internal: the section owns bytes and can participate in internal light
  reads, but is omitted from vanilla packet conversion. Non-zero hidden data is
  currently persisted in Steel's format as hidden; all-zero hidden data
  canonicalizes to omitted.

This is a good candidate Rust storage shape:

```rust
enum LightSection {
    Missing,
    Zero,
    Data(Box<[u8; DATA_LAYER_SIZE]>),
    Internal(Box<[u8; DATA_LAYER_SIZE]>),
}
```

Names are not final. The important property is that missing, present-zero,
present-data, and internal-only are type-level states.

### Packet Conversion

Match vanilla `ClientboundLightUpdatePacketData.prepareSectionData`:

- Missing/internal sections are omitted.
- Present zero sections set the empty mask.
- Present non-zero data sections set the update mask and append bytes.
- Updates are emitted in ascending light-section-index order.
- Sky data is omitted entirely when the dimension has no skylight.

### Persistence

- Null/missing sections are omitted.
- Present zero sections persist as uninitialized.
- Present data sections persist packed bytes.
- Internal/hidden non-zero sections persist as internal/hidden Steel light data.
- All-zero data sections canonicalize to present zero.
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
- Fresh light generation:
  - Requires center chunk at `InitializeLight`.
  - Reads center and initialized neighbors.
  - Writes center and already-lit neighbors only.
  - Runs sky first when the dimension has skylight, then block light.
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
- Dynamic propagation uses a full-radius workset and runs sky changes before
  block changes.

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
- Increase and decrease queues are separate FIFO queues.
- Decrease processing can enqueue increases, then increase processing runs.
- Edge checks collect delayed local indices and then run regular block checks.

### Cache Window

The current reference uses:

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

## Subtle Improvements Allowed

The rewrite may improve structure if behavior remains pinned:

- Rename `Null`, `Uninitialized`, `Initialized`, `Hidden` to domain terms.
- Move visible/updating state out of chunk-owned sections into scoped edits.
- Hide packed ScalableLux queue bits behind typed constructors.
- Replace temporary "removed null nibble" behavior with a typed transient edit
  state, as long as transient sections still do not commit to chunk storage.
- Group block-light and sky-light shared edge/queue helpers if tests stay clear.
- Keep comments about vanilla/ScalableLux only where they explain a preserved
  behavioral constraint.

## Required Parity Tests

Before replacing the current branch, keep or add tests for:

- Light section range padding for normal and overworld heights.
- Packet masks for missing, zero, data, hidden/internal, filtered sections, and no-skylight dimensions.
- Save/load roundtrip for zero, data, hidden/internal, and loaded sky normalization.
- World light reads for block light, sky light upward search, and full sky above highest non-empty.
- `ChunkSkyLightSources` fill/update behavior.
- Dynamic block changes for emission, opacity, section empty transitions, and sky-source changes.
- Fresh worldgen light for sky and block light.
- Loaded light preserving interior data while validating edges.
- Block edge checks pulling missing neighbor light and removing stale center light.
- Sky edge checks pulling neighbor light under ceilings and skipping null center extrusion.
- Direction order, queue flags, packed queue entries, and FIFO behavior if packed queues remain.

Recommended high-level parity harness:

- Generate a small fixed set of chunks on the reference branch.
- Record chunk light packets and saved light data hashes.
- Run the same seeds/dimensions on the redesigned branch.
- Compare both section update sets and final visible packet data.

## Open Decisions

- Should Steel persist hidden/internal light data long term, or can hidden data
  become a purely transient edit state? Current behavior persists non-zero
  hidden sections.
- Should the new chunk-owned storage use a dense array over the padded light
  range, or sparse storage keyed by section position like vanilla? Dense storage
  is simpler for current chunk ownership; sparse storage is closer to vanilla
  and may avoid representing missing sections explicitly.
- Should packed ScalableLux queue encoding remain exactly as-is internally?
  It is not externally visible, but changing it risks subtle order differences.
- Should block and sky propagation share a generic propagation context, or stay
  separate with duplicated edge helpers for readability?
- What should the new names be for missing/present-zero/data/internal states?

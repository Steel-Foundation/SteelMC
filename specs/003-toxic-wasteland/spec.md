# Feature Specification: Toxic Wasteland (custom fluids and biomes for vanilla clients)

**Feature Branch**: `003-toxic-wasteland`

**Created**: 2026-11-01

**Status**: Draft

**Input**: User description: "Toxic sludge biome extension for SteelExtensionMC (vanilla clients, no mod, no resource pack). New extension `toxic-wasteland` plus the reusable platform support it needs: a custom biome with green water and fog, a custom 'toxic sludge' fluid that is always rendered green, a dedicated wasteland world with sludge lakes, an admin command to paint the biome and pour or drain sludge in any world, and gameplay (poison and damage, a bucket, water corruption, plant killing), all configurable."

Decisions taken before writing (asked of the user):

- The biome exists in a dedicated, extension-generated **wasteland world** and can be **painted** onto regions of any world by an administrator. It does not appear in vanilla overworld generation.
- **The tint follows the sludge**: wherever sludge is, clients see it green, even outside the toxic biome, while the server keeps the real biome.
- Gameplay includes **poison and damage, a sludge bucket, water corruption, and plant killing**.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Sludge is a real fluid that looks green and behaves like a liquid (Priority: P1)

A player with an unmodified Minecraft client finds a pool of toxic sludge. It looks like green water, flows downhill like a liquid, and they can swim in it without being pulled back or stuttering. When the server restarts, the sludge is still sludge, not ordinary water.

**Why this priority**: the custom fluid is the foundation of everything else, and the reason the feature exists. Without it, there is nothing to tint, generate, or pour.

**Independent Test**: with only the extension installed, an operator places a sludge source with the admin command in a plain overworld. Observe its color and flow, swim in it, restart the server, and check that it is still sludge.

**Acceptance Scenarios**:

1. **Given** a sludge source placed on flat ground, **When** time passes, **Then** sludge spreads into flowing sludge following the extension's spread rules (reach and speed may differ from water) and stops at its configured maximum reach.
2. **Given** a vanilla client inside sludge, **When** the player swims, sinks, and climbs out, **Then** the movement is identical to water on both client and server, with no position corrections.
3. **Given** a world containing sludge, **When** the server is stopped and started again, **Then** every sludge block is restored as sludge, at the same level.
4. **Given** sludge in any biome, **When** a vanilla client looks at it, **Then** it is rendered in the configured sludge color, never the surrounding biome's water color.
5. **Given** all sludge in an area is removed, **When** the client next looks, **Then** water and grass there show their real biome colors again without the player reconnecting.

---

### User Story 2 - A toxic biome that clients see (Priority: P1)

An operator paints the "toxic wasteland" biome over a region of an existing world. Players in that region see a sickly sky, fog, grass, and foliage, and green water. `/locate biome` and the F3 screen name the new biome.

**Why this priority**: custom biomes are the second reusable building block. The wasteland world and the paint command both depend on it.

**Independent Test**: register the biome, paint a region with the admin command, and observe the colors in a vanilla client. Restart the server and confirm the biome remains.

**Acceptance Scenarios**:

1. **Given** the extension is installed, **When** a vanilla client joins, **Then** it receives the toxic biome with its configured colors and accepts it without errors.
2. **Given** an operator paints the biome over a region, **When** players are nearby, **Then** their view updates to the new biome without reloading the chunks or reconnecting.
3. **Given** a painted region, **When** the server restarts, **Then** the region is still the toxic biome.
4. **Given** the biome's configured colors are changed, **When** the server restarts, **Then** clients see the new colors.

---

### User Story 3 - Sludge is dangerous (Priority: P2)

A player who wades into sludge is poisoned and takes damage while they stay in it. Sludge spreading over farmland kills the crops, nearby grass slowly withers to coarse dirt, and a lake of sludge slowly corrupts adjoining water into more sludge.

**Why this priority**: this turns a colored liquid into a hazard. It depends on story 1 but not on the world or the biome.

**Independent Test**: place sludge next to a player, a crop field, grass, and a water pool, then measure the effects over time against the configured rates.

**Acceptance Scenarios**:

1. **Given** a living entity touching sludge, **When** it stays in contact, **Then** it is poisoned for the configured duration and takes the configured damage at the configured interval. Leaving the sludge stops the extra damage, though remaining poison runs its course.
2. **Given** an entity immune to poison (for example undead mobs) in sludge, **When** in contact, **Then** it is not poisoned, but it still takes the configured contact damage.
3. **Given** creative or spectator players, **When** in sludge, **Then** they take no damage and receive no effect.
4. **Given** flowing sludge reaching a crop, flower, sapling, or tall grass, **When** it flows into that block, **Then** the plant is destroyed (with its drops, as water does) and sludge takes its place.
5. **Given** grass, podzol, mycelium, or dirt directly beside or below sludge, **When** random ticks occur, **Then** that block eventually turns into coarse dirt at the configured rate.
6. **Given** a water source or flowing water beside sludge, **When** random ticks occur, **Then** the water is eventually converted into sludge of the same level at the configured rate. A lava/sludge contact follows a defined rule (see FR-014).
7. **Given** corruption or withering is disabled in the configuration, **When** time passes, **Then** that effect never happens.

---

### User Story 4 - A sludge bucket (Priority: P2)

A player uses an empty bucket on a sludge source and gets a "Bucket of Toxic Sludge". Using it places a sludge source, and the bucket becomes empty again, just like a water bucket. Creative players keep their bucket.

**Why this priority**: it lets players move the hazard around and is the natural survival interaction with any fluid. It depends on story 1.

**Independent Test**: pick up and pour sludge in survival and creative, then check inventory contents and the placed blocks.

**Acceptance Scenarios**:

1. **Given** a survival player with an empty bucket facing a sludge source, **When** they use it, **Then** the source is removed and the player gains a sludge bucket (with the vanilla pickup sound).
2. **Given** a sludge bucket, **When** used on a valid target, **Then** a sludge source is placed and the bucket becomes empty (survival) or is kept (creative).
3. **Given** a sludge bucket used where water would evaporate (the nether), **When** used, **Then** it behaves as a water bucket does there (evaporates with smoke, nothing placed).
4. **Given** flowing (non-source) sludge, **When** an empty bucket is used on it, **Then** nothing is picked up.
5. **Given** a sludge bucket in an inventory, **When** the server restarts, **Then** it is still a sludge bucket.
6. **Given** a sludge bucket used on a waterloggable block, **When** used, **Then** the block is not waterlogged with sludge; sludge is placed in front of it, or the use fails as for a non-replaceable block (see Assumptions).

---

### User Story 5 - The wasteland world (Priority: P3)

The server owner adds a "wasteland" world to the configuration. Its terrain is a barren landscape, entirely in the toxic biome, scattered with sludge lakes. Players teleport there and explore it.

**Why this priority**: this is the showcase. It depends on stories 1 and 2 and on the existing extension generator support.

**Independent Test**: configure a world with the extension's generator, generate the area around spawn, and check that the biome is everywhere and that sludge lakes appear at the expected frequency.

**Acceptance Scenarios**:

1. **Given** a world configured with the wasteland generator, **When** chunks generate, **Then** every biome cell is the toxic biome and the terrain is varied (hills, flats, lowlands).
2. **Given** generated terrain, **When** a player explores 512×512 blocks, **Then** sludge lakes appear at the configured frequency, contained by terrain without spilling at generation time.
3. **Given** the same seed, **When** the world is generated twice, **Then** the terrain and lakes are identical.

---

### User Story 6 - Admin paint and pour tools (Priority: P3)

An operator stands in any world and runs commands to paint the toxic biome over a box or their WorldEdit selection, pour sludge into an area, or drain all sludge from an area.

**Why this priority**: it brings the content into existing worlds without generation. It depends on stories 1 and 2.

**Independent Test**: run each command on a known region and inspect the biome cells and blocks there.

**Acceptance Scenarios**:

1. **Given** an operator with a WorldEdit selection, **When** they paint the biome without coordinates, **Then** the selection is painted. With explicit coordinates, those are used, and WorldEdit is not required.
2. **Given** a paint request, **When** it finishes, **Then** the operator is told how many biome cells changed, and nearby players see the change at once.
3. **Given** a pour request on a region, **When** run, **Then** air and replaceable blocks in the region become sludge sources (solid blocks untouched). A drain request removes all sludge (source and flowing) in the region.
4. **Given** a non-operator, **When** they run these commands, **Then** they are refused with the standard permission message.
5. **Given** a region larger than the configured maximum, **When** a command is run, **Then** it is refused with a message stating the limit.
6. **Given** a world that exists, **When** an operator paints a biome other than the toxic biome (any registered biome key, such as `minecraft:plains`), **Then** it is painted too, so painting is reversible.

### Edge Cases

- Sludge and plain water in the same 4×4×4 biome cell: the water also appears green. This is an accepted limitation; the server's gameplay still treats it as water.
- A player stands in sludge at the moment it is drained: damage stops on the next check; no damage is dealt for blocks that are no longer sludge.
- Sludge meets lava: see FR-014. Sludge flowing onto lava sources or flowing lava follows the rule, never leaves an invisible or undefined block.
- Clients that join while sludge already exists see the tinted cells in the first chunk data, not after a delayed refresh.
- Many sludge blocks change in one tick (pouring a large region, a lake draining): tint updates are batched, so clients receive at most one biome refresh per affected chunk per tick.
- The extension is removed while worlds contain sludge or the toxic biome: the server refuses to load those chunks with a clear message (it does not silently turn them into water or plains), consistent with the existing custom content rules.
- Two extensions register custom fluids or biomes with the same key: registration fails with an error naming the duplicate key.
- A custom biome's colors are invalid (out of range): registration fails with a clear message.
- The sludge bucket is used through a dispenser: optional for this feature; if unsupported, the dispenser drops the bucket as an item (vanilla behavior for unknown items) and the limitation is documented.
- Sludge reaching the world's bottom or top: it obeys the same world limits as water.
- An entity in sludge that is also in water (cell boundary): it takes sludge effects only if its body is in a sludge block.
- Painting the biome in a chunk that is not loaded: the command loads (or refuses, with a message) according to the configured maximum; it never silently skips cells.

## Requirements *(mandatory)*

### Functional Requirements

**Reusable platform: custom fluids**

- **FR-001**: Extensions MUST be able to register a new fluid, with source and flowing variants, its own liquid block, and an optional bucket item, during startup content registration, before registries are frozen.
- **FR-002**: A registered fluid MUST define the vanilla fluid that clients see it as (water or lava). That choice also determines the movement physics applied to entities on the server, so that client prediction and server movement agree.
- **FR-003**: A registered fluid MUST be able to customize its spread behavior: tick delay, level drop per block, slope search distance, whether it forms new sources, and what it may replace.
- **FR-004**: Saved worlds MUST store custom fluid blocks by their own registry key and level, and restore them on load. Clients MUST only ever receive vanilla block states for them.
- **FR-005**: Extensions MUST be able to react to entities being inside their fluid, to the fluid flowing into or destroying a block, and to random ticks of their fluid, through a typed callback interface.

**Reusable platform: custom biomes**

- **FR-006**: Extensions MUST be able to register new biomes with full client-visible appearance (sky, fog, water, water fog, grass, foliage, and optional dry-foliage colors, grass color modifier, precipitation, temperature, downfall, ambient particles, and music) during startup registration. The biome MUST be sent to every client and saved by its key.
- **FR-007**: Extensions MUST be able to change the biome of a region of a loaded world at runtime. Players who can see that region MUST receive the change without reloading the chunks, and the change MUST persist.
- **FR-008**: Extensions MUST be able to read the biome at a position.
- **FR-009**: A registered fluid MUST be able to declare a **client water tint**. When set, every 4×4×4 biome cell that contains that fluid is presented to clients as a tinted variant of that cell's real biome, identical except for its water and water-fog colors. The server's gameplay and saved data keep the real biome. The variant set MUST cover every registered biome, including other extensions' biomes.
- **FR-010**: Tinted presentation MUST be correct in a client's first view of a chunk and MUST update as the fluid appears in or leaves a cell. Updates in one tick MUST be coalesced to at most one refresh per chunk per player.
- **FR-011**: Extensions MUST be able to register a terrain generator whose biome is a custom biome (extends the existing extension generator support).

**Toxic Wasteland extension**

- **FR-012**: The extension MUST register the `toxic_wasteland:wasteland` biome and the `toxic_wasteland:sludge` fluid (displayed as water, tinted with the sludge color), its block, and the "Bucket of Toxic Sludge" item.
- **FR-013**: Sludge MUST spread more slowly than water by default (configurable tick delay and reach) and MUST NOT form new infinite sources by default (configurable).
- **FR-014**: When flowing sludge meets lava, the lava's position becomes coarse dirt for a lava source, or cobblestone for flowing lava, with the vanilla fizz sound. Sludge flowing onto lava MUST NOT leave a mixed or undefined state.
- **FR-015**: Living entities in contact with sludge MUST receive the configured poison effect (respecting effect immunity) and contact damage at the configured interval. Creative and spectator players are exempt.
- **FR-016**: Flowing sludge MUST destroy the vanilla blocks water destroys (plants, crops, torches, and similar), with their drops, before occupying the position.
- **FR-017**: Sludge sources and flowing sludge MUST, on random ticks at the configured rate, convert an adjacent water block into sludge of the same level and turn an adjacent or underlying grass, dirt, podzol, or mycelium block into coarse dirt. Each conversion MUST be independently disableable.
- **FR-018**: The bucket MUST support pick-up from sludge sources and pouring, matching vanilla water bucket behavior for survival, creative, sounds, nether evaporation, and replaceable targets.
- **FR-019**: The extension MUST provide a `wasteland` terrain generator with varied barren terrain, the toxic biome everywhere, and seed-deterministic sludge lakes at a configured frequency.
- **FR-020**: The extension MUST provide operator commands to paint any registered biome, pour sludge, and drain sludge over an explicit box or the operator's WorldEdit selection, if WorldEdit is installed (soft dependency). Each command reports the affected count and refuses regions over a configured volume limit.
- **FR-021**: All tunables MUST come from `extensions/toxic-wasteland/config.toml`, with documented defaults, validated at startup. An invalid value MUST prevent the extension from loading, with a message naming the field.

**Quality**

- **FR-022**: Behavior with no extension registering fluids or biomes MUST be unchanged, including byte-identical chunk and registry data sent to clients (vanilla parity).
- **FR-023**: The new platform interfaces MUST be documented in `doc/extensions`, with a working example. The extension MUST have a README.
- **FR-024**: Fluid registration, persistence round-trips, tint projection (first view and live updates), biome painting, and the extension's gameplay rules MUST be covered by automated tests, including native integration tests that load the built extension.

### Key Entities

- **Custom fluid**: a server identity (key), source and flowing variants, its liquid block (levels 0–15), a client appearance (vanilla water or lava, optional tint color), spread rules, and behavior callbacks owned by the registering extension.
- **Custom biome**: a key with climate (temperature, downfall, precipitation) and client appearance (colors, particles, music), saved in chunks by key.
- **Tinted biome variant**: a client-only copy of a real biome, with a fluid's tint water colors. It is never saved and never seen by server gameplay; it exists for every registered biome and tinting fluid.
- **Biome cell**: the 4×4×4 unit in which biomes are stored. It tracks whether it contains a tinting fluid, to decide its client presentation.
- **Toxic Wasteland configuration**: colors, damage and poison values, spread values, corruption and withering rates, lake frequency, and the command volume limit.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A vanilla client with no mods or resource packs sees sludge in the sludge color in 100% of the places it exists, in any biome, from its first view of the area.
- **SC-002**: Swimming in sludge for 60 seconds causes zero server movement corrections, the same as in water.
- **SC-003**: After a restart, 100% of sludge blocks, painted biome cells, and sludge buckets are restored exactly.
- **SC-004**: A painted or tint change appears for nearby players within one second, without reconnecting or reloading the chunks.
- **SC-005**: Painting the biome over a 256×256×384 region completes in under 2 seconds on the reference machine, and pouring sludge over a 64×64×16 region completes within one server tick budget plus normal spreading.
- **SC-006**: A server with the extension installed and no sludge in view sends no more data per chunk than a server without it.
- **SC-007**: In a 512×512 area of the wasteland world, sludge lakes cover between 2% and 15% of the surface at default settings, and identical seeds produce identical worlds.
- **SC-008**: All gameplay effects (poison, damage, plant destruction, withering, corruption) happen at their configured rates within ±10% over 10,000 simulated random ticks.

## Assumptions

- Sludge is displayed as water, so it shares water's client physics (swimming, buoyancy, drowning, extinguishing). Lava-like viscosity is out of scope, because vanilla clients would mispredict it.
- Sludge does **not** waterlog blocks. Waterlogging stores a vanilla boolean that clients render as water in the biome's color, and server-side waterlogging is tied to water specifically. Sludge buckets used on waterloggable blocks place sludge as for a non-waterloggable target. This may be revisited later.
- Particles, sounds, block-breaking cracks, and the underwater overlay follow the water the client sees. Only colors change through the tint.
- Tinted variants double the size of the biome registry sent to clients once at login (about 65 vanilla biomes plus custom ones, per tinting fluid). This is a one-time cost of a few kilobytes and acceptable.
- Mobs and spawning in the wasteland biome: no natural spawns by default, as a barren hazard zone. Mob spawning configuration is out of scope.
- The vanilla overworld is unchanged. The biome appears only in the wasteland world and where painted.
- Dispenser support for the sludge bucket is optional and may ship as a documented limitation.
- The WorldEdit integration uses the existing `worldedit:selection/v1` service and is a soft dependency.
- The reference machine for performance is the development machine used for previous iterations' benchmarks (the 94 ms per one-million-block bulk fill).

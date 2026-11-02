# Contract: `/toxic` command (toxic-wasteland extension)

Requires operator level 2. The volume limit is `commands.max_volume`.

| Syntax | Effect | Feedback |
| --- | --- | --- |
| `/toxic paint <biome>` | Paints the executor's WorldEdit selection. | `Painted N biome cells as <biome>.` |
| `/toxic paint <biome> <from> <to>` | Paints the explicit box. | Same. |
| `/toxic pour [<from> <to>]` | Turns air and replaceable blocks into sludge sources. | `Poured N sludge blocks.` |
| `/toxic drain [<from> <to>]` | Turns all sludge (source and flowing) into air. | `Drained N sludge blocks.` |

Errors:
- No selection and no coordinates (or WorldEdit absent): `Make a WorldEdit selection or give two corners.`
- Over the volume limit: `Region has V blocks; the limit is L.`
- Unknown biome: the standard argument error.
- Unloaded chunks in the region: `K chunks in the region are not loaded.`

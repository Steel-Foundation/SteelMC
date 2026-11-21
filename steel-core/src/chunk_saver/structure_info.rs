use std::io;

use rustc_hash::FxHashMap;
use steel_utils::{ChunkPos, Identifier};

use super::{ChunkStorage, PersistentChunk};
use crate::chunk::status::ChunkStatus;

/// Reference counts of valid starts in a chunk whose structure stage is complete.
/// This snapshot does not construct chunk sections, entities, or structure pieces.
#[derive(Debug, PartialEq, Eq)]
pub struct ChunkStructureInfo {
    starts: FxHashMap<Identifier, i32>,
}

impl ChunkStructureInfo {
    pub(super) fn from_persistent(data: &PersistentChunk<'_>, status: ChunkStatus) -> Option<Self> {
        (status >= ChunkStatus::StructureStarts).then(|| Self {
            starts: data
                .structure_starts
                .iter()
                .filter(|start| !start.pieces.is_empty())
                .map(|start| (start.structure.clone(), start.references))
                .collect(),
        })
    }

    /// Whether a valid start satisfies the requested reference constraint.
    #[must_use]
    pub fn contains(&self, structure: &Identifier, require_unreferenced: bool) -> bool {
        self.starts
            .get(structure)
            .is_some_and(|references| !require_unreferenced || *references == 0)
    }
}

impl ChunkStorage {
    /// Reads saved start metadata without loading a gameplay chunk. `None` means
    /// the saved data cannot establish whether a start exists.
    pub async fn read_structure_info(
        &self,
        pos: ChunkPos,
        thread_pool: &rayon::ThreadPool,
    ) -> io::Result<Option<ChunkStructureInfo>> {
        match self {
            Self::Disk(storage) => storage.read_structure_info(pos, thread_pool).await,
            Self::RamOnly(storage) => Ok(storage.read_structure_info(pos).await),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{env::temp_dir, fs, sync::Weak};

    use super::*;
    use crate::{
        chunk::{
            Chunk,
            section::{ChunkSection, Sections},
        },
        chunk_saver::{RamOnlyStorage, RegionManager},
    };
    use steel_registry::{init_vanilla_registry, vanilla_structures};

    #[tokio::test]
    async fn structure_scans_distinguish_unknown_from_completed_absence_in_both_backends() {
        init_vanilla_registry();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("decode pool");
        for disk in [false, true] {
            let root = temp_dir().join(format!("steel-structure-info-{}", uuid::Uuid::new_v4()));
            let storage = if disk {
                ChunkStorage::Disk(RegionManager::new(&root))
            } else {
                ChunkStorage::RamOnly(RamOnlyStorage::empty_world())
            };
            let pos = ChunkPos::new(-2, 3);
            assert!(
                storage
                    .read_structure_info(pos, &pool)
                    .await
                    .expect("missing chunk")
                    .is_none()
            );
            assert!(
                !root.exists(),
                "metadata reads must not create storage files"
            );
            storage.acquire_chunk(pos).await.expect("acquire region");
            let chunk = Chunk::new(
                Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
                pos,
                0,
                16,
                Weak::new(),
            );
            for status in [ChunkStatus::Empty, ChunkStatus::StructureStarts] {
                let prepared =
                    ChunkStorage::prepare_chunk_save(&chunk, status, &[], true).expect("snapshot");
                storage
                    .save_chunk_data(prepared, &pool)
                    .await
                    .expect("save");
                let info = storage
                    .read_structure_info(pos, &pool)
                    .await
                    .expect("scan cached region");
                if status == ChunkStatus::Empty {
                    assert!(info.is_none());
                } else {
                    assert!(
                        !info
                            .expect("completed starts")
                            .contains(&vanilla_structures::STRUCTURE_BURIED_TREASURE.key, false)
                    );
                }
            }
            storage.release_chunk(pos).await.expect("release region");
            assert!(
                storage
                    .read_structure_info(pos, &pool)
                    .await
                    .expect("scan uncached region")
                    .is_some()
            );
            if disk {
                fs::remove_dir_all(root).expect("remove fixture");
            }
        }
    }
}

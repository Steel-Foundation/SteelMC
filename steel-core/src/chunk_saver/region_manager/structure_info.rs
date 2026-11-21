use std::io;

use steel_utils::ChunkPos;
use tokio::{fs::File, io::AsyncReadExt, sync::oneshot};

use super::RegionManager;
use crate::{
    chunk::status::ChunkStatus,
    chunk_saver::{
        ChunkStructureInfo, PersistentChunk,
        format::{
            CHUNK_TABLE_SIZE, FILE_HEADER_SIZE, FORMAT_VERSION, REGION_MAGIC, RegionHeader,
            RegionPos, SECTOR_SIZE,
        },
    },
};

impl RegionManager {
    /// Scans structure metadata using an owned payload, with no region lease or
    /// gameplay chunk. Cancelled reads therefore cannot retain loading tickets.
    pub async fn read_structure_info(
        &self,
        pos: ChunkPos,
        thread_pool: &rayon::ThreadPool,
    ) -> io::Result<Option<ChunkStructureInfo>> {
        let Some((compressed, status)) = self.structure_payload(pos).await? else {
            return Ok(None);
        };
        let (sender, receiver) = oneshot::channel();
        thread_pool.spawn(move || {
            let result = Self::decode_structure_info(&compressed, status);
            if sender.send(result).is_err() {
                tracing::trace!(chunk = ?pos, "Discarding cancelled structure metadata scan");
            }
        });
        receiver
            .await
            .map_err(|_| io::Error::other("structure metadata decoder ended without a result"))?
    }

    async fn structure_payload(&self, pos: ChunkPos) -> io::Result<Option<(Vec<u8>, ChunkStatus)>> {
        let region = RegionPos::from_chunk(pos.0.x, pos.0.y);
        let (x, z) = RegionPos::local_chunk_pos(pos.0.x, pos.0.y);
        let index = RegionHeader::chunk_index(x, z);
        // Serializes the read with region writes even when the file is not cached.
        let mut regions = self.regions.write().await;
        if let Some(handle) = regions.get_mut(&region) {
            let entry = handle.header.entries[index];
            if !entry.exists() || entry.status < ChunkStatus::StructureStarts {
                return Ok(None);
            }
            Self::validate_chunk_entry(entry, handle.file_sectors)?;
            let bytes =
                Self::read_chunk_data(&mut handle.file, entry.sector_offset, entry.size_bytes)
                    .await?;
            return Ok(Some((bytes, entry.status)));
        }
        let mut file = match File::open(self.region_path(region)).await {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut prefix = [0; FILE_HEADER_SIZE];
        file.read_exact(&mut prefix).await?;
        if prefix[..4] != REGION_MAGIC
            || u16::from_le_bytes([prefix[4], prefix[5]]) != FORMAT_VERSION
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported region header in structure scan",
            ));
        }
        let mut table = vec![0; CHUNK_TABLE_SIZE];
        file.read_exact(&mut table).await?;
        let header = RegionHeader::from_bytes(&table).map_err(|index| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid region chunk entry {index}"),
            )
        })?;
        let size = file.metadata().await?.len();
        let sectors = u32::try_from(size.div_ceil(SECTOR_SIZE as u64)).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "region file exceeds sector range",
            )
        })?;
        Self::validate_region_entries(&header, sectors)?;
        let entry = header.entries[index];
        if !entry.exists() || entry.status < ChunkStatus::StructureStarts {
            return Ok(None);
        }
        let bytes = Self::read_chunk_data(&mut file, entry.sector_offset, entry.size_bytes).await?;
        Ok(Some((bytes, entry.status)))
    }

    fn decode_structure_info(
        compressed: &[u8],
        status: ChunkStatus,
    ) -> io::Result<Option<ChunkStructureInfo>> {
        let data = zstd::decode_all(compressed)?;
        let persistent: PersistentChunk<'_> = wincode::deserialize(&data)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(ChunkStructureInfo::from_persistent(&persistent, status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{
        Chunk,
        section::{ChunkSection, Sections},
    };
    use crate::chunk_saver::ChunkStorage;
    use std::{env::temp_dir, sync::Weak};
    use steel_registry::init_vanilla_registry;
    use tokio::{
        fs,
        io::{AsyncSeekExt, AsyncWriteExt},
    };

    #[tokio::test]
    async fn a_failed_structure_scan_leaves_corrupt_payloads_for_normal_chunk_recovery() {
        init_vanilla_registry();
        let root = temp_dir().join(format!(
            "steel-structure-corruption-{}",
            uuid::Uuid::new_v4()
        ));
        let manager = RegionManager::new(&root);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("decode pool");
        let pos = ChunkPos::new(0, 0);
        manager.acquire_chunk(pos).await.expect("region");
        let chunk = Chunk::new(
            Sections::from_owned(vec![ChunkSection::new_empty()].into_boxed_slice()),
            pos,
            0,
            16,
            Weak::new(),
        );
        let prepared =
            ChunkStorage::prepare_chunk_save(&chunk, ChunkStatus::StructureStarts, &[], true)
                .expect("snapshot");
        manager
            .save_chunk_data(prepared, &pool)
            .await
            .expect("save");
        {
            let mut regions = manager.regions.write().await;
            let handle = regions
                .get_mut(&RegionPos::from_chunk(0, 0))
                .expect("cached region");
            let entry = handle.header.entries[0];
            handle
                .file
                .seek(io::SeekFrom::Start(
                    u64::from(entry.sector_offset) * SECTOR_SIZE as u64,
                ))
                .await
                .expect("seek");
            handle
                .file
                .write_all(&[0; 4])
                .await
                .expect("corrupt compressed magic");
            handle.file.flush().await.expect("flush");
        }
        assert!(manager.read_structure_info(pos, &pool).await.is_err());
        assert!(manager.chunk_exists(pos).await.expect("slot is retained"));
        assert!(
            manager
                .load_chunk(pos, 0, 16, Weak::new(), &pool)
                .await
                .expect("normal recovery")
                .is_none()
        );
        assert!(
            !manager
                .chunk_exists(pos)
                .await
                .expect("normal recovery clears corruption")
        );
        manager.release_chunk(pos).await.expect("release");
        fs::remove_dir_all(root).await.expect("remove fixture");
    }
}

#![expect(missing_docs, reason = "benchmarks")]

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, SamplingMode, criterion_group, criterion_main};
use steel_core::chunk::simulation_benchmark_support::SimulationTicketManager;
use steel_utils::ChunkPos;
use uuid::Uuid;

const SOURCE_COUNTS: [usize; 4] = [1, 10, 100, 500];
const SIMULATION_DISTANCES: [u8; 3] = [2, 10, 32];
const DISPERSED_SOURCE_SPACING: i32 = 80;

#[derive(Clone, Copy)]
enum SourceLayout {
    Clustered,
    Dispersed,
}

impl SourceLayout {
    const fn name(self) -> &'static str {
        match self {
            Self::Clustered => "clustered",
            Self::Dispersed => "dispersed",
        }
    }

    const fn position(self, index: usize) -> ChunkPos {
        let column = index as i32 % 25;
        let row = index as i32 / 25;
        let spacing = match self {
            Self::Clustered => 1,
            Self::Dispersed => DISPERSED_SOURCE_SPACING,
        };
        ChunkPos::new(column * spacing, row * spacing)
    }

    const fn moved_position(self) -> ChunkPos {
        match self {
            Self::Clustered => ChunkPos::new(-1, 0),
            Self::Dispersed => ChunkPos::new(-DISPERSED_SOURCE_SPACING, 0),
        }
    }
}

struct MoveScenario {
    manager: SimulationTicketManager,
    player_id: Uuid,
    original_pos: ChunkPos,
    moved_pos: ChunkPos,
    player_is_moved: bool,
}

impl MoveScenario {
    fn new(layout: SourceLayout, source_count: usize, simulation_distance: u8) -> Self {
        let mut manager = SimulationTicketManager::new();
        for index in 0..source_count {
            let player_id = player_id(index);
            assert!(manager.add_player(layout.position(index), player_id));
        }
        black_box(manager.run_all_updates(simulation_distance));

        Self {
            manager,
            player_id: player_id(0),
            original_pos: layout.position(0),
            moved_pos: layout.moved_position(),
            player_is_moved: false,
        }
    }

    fn move_player_and_rebuild(&mut self, simulation_distance: u8) -> usize {
        let (old_pos, new_pos) = if self.player_is_moved {
            (self.moved_pos, self.original_pos)
        } else {
            (self.original_pos, self.moved_pos)
        };

        assert!(self.manager.remove_player(old_pos, self.player_id));
        assert!(self.manager.add_player(new_pos, self.player_id));
        self.player_is_moved = !self.player_is_moved;

        self.manager.run_all_updates(simulation_distance).len()
    }
}

const fn player_id(index: usize) -> Uuid {
    Uuid::from_u128(index as u128 + 1)
}

fn bench_simulation_ticket_updates(c: &mut Criterion) {
    for layout in [SourceLayout::Clustered, SourceLayout::Dispersed] {
        for simulation_distance in SIMULATION_DISTANCES {
            let mut group = c.benchmark_group(format!(
                "simulation_ticket_updates/{}/distance_{simulation_distance}",
                layout.name()
            ));
            group.sample_size(10);
            group.sampling_mode(SamplingMode::Flat);
            group.warm_up_time(Duration::from_millis(250));
            group.measurement_time(Duration::from_millis(750));

            for source_count in SOURCE_COUNTS {
                let mut scenario = MoveScenario::new(layout, source_count, simulation_distance);
                group.bench_with_input(
                    BenchmarkId::from_parameter(source_count),
                    &source_count,
                    |b, _| {
                        b.iter(|| {
                            black_box(scenario.move_player_and_rebuild(simulation_distance));
                        });
                    },
                );
            }

            group.finish();
        }
    }
}

criterion_group!(benches, bench_simulation_ticket_updates);
criterion_main!(benches);

#![expect(missing_docs, reason = "benchmarks")]

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, SamplingMode, criterion_group, criterion_main};
use steel_core::chunk::simulation_benchmark_support::SimulationTicketBenchmarkScenario;
use steel_utils::ChunkPos;
use uuid::Uuid;

const SOURCE_COUNTS: [usize; 4] = [1, 10, 100, 500];
const SIMULATION_DISTANCES: [u8; 3] = [2, 10, 32];
const SOURCE_GRID_WIDTH: i32 = 25;
const CLUSTERED_SOURCE_SPACING: i32 = 1;
const DISPERSED_SOURCE_SPACING: i32 = 80;
const BENCHMARK_SAMPLE_SIZE: usize = 10;
const BENCHMARK_WARM_UP: Duration = Duration::from_millis(250);
const BENCHMARK_MEASUREMENT: Duration = Duration::from_millis(750);

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

    fn position(self, index: usize) -> ChunkPos {
        let index = i32::try_from(index).expect("benchmark source index must fit in i32");
        let column = index % SOURCE_GRID_WIDTH;
        let row = index / SOURCE_GRID_WIDTH;
        ChunkPos::new(column * self.spacing(), row * self.spacing())
    }

    const fn spacing(self) -> i32 {
        match self {
            Self::Clustered => CLUSTERED_SOURCE_SPACING,
            Self::Dispersed => DISPERSED_SOURCE_SPACING,
        }
    }

    const fn moved_position(self) -> ChunkPos {
        ChunkPos::new(-self.spacing(), 0)
    }
}

struct MoveScenario {
    benchmark: SimulationTicketBenchmarkScenario,
    player_id: Uuid,
    original_pos: ChunkPos,
    moved_pos: ChunkPos,
    player_is_moved: bool,
}

impl MoveScenario {
    fn new(layout: SourceLayout, source_count: usize, simulation_distance: u8) -> Self {
        let players = (0..source_count).map(|index| (player_id(index), layout.position(index)));
        let benchmark = SimulationTicketBenchmarkScenario::new(simulation_distance, players);

        Self {
            benchmark,
            player_id: player_id(0),
            original_pos: layout.position(0),
            moved_pos: layout.moved_position(),
            player_is_moved: false,
        }
    }

    fn move_player_and_propagate(&mut self) -> usize {
        let (old_pos, new_pos) = if self.player_is_moved {
            (self.moved_pos, self.original_pos)
        } else {
            (self.original_pos, self.moved_pos)
        };

        self.player_is_moved = !self.player_is_moved;

        self.benchmark.move_player(self.player_id, old_pos, new_pos)
    }
}

fn player_id(index: usize) -> Uuid {
    let index = u128::try_from(index).expect("benchmark source index must fit in u128");
    Uuid::from_u128(index + 1)
}

fn bench_simulation_ticket_pipeline(c: &mut Criterion) {
    for layout in [SourceLayout::Clustered, SourceLayout::Dispersed] {
        for simulation_distance in SIMULATION_DISTANCES {
            let mut group = c.benchmark_group(format!(
                "simulation_ticket_pipeline/{}/distance_{simulation_distance}",
                layout.name()
            ));
            group.sample_size(BENCHMARK_SAMPLE_SIZE);
            group.sampling_mode(SamplingMode::Flat);
            group.warm_up_time(BENCHMARK_WARM_UP);
            group.measurement_time(BENCHMARK_MEASUREMENT);

            for source_count in SOURCE_COUNTS {
                let mut scenario = MoveScenario::new(layout, source_count, simulation_distance);
                group.bench_with_input(
                    BenchmarkId::from_parameter(source_count),
                    &source_count,
                    |b, _| {
                        b.iter(|| {
                            black_box(scenario.move_player_and_propagate());
                        });
                    },
                );
            }

            group.finish();
        }
    }
}

criterion_group!(benches, bench_simulation_ticket_pipeline);
criterion_main!(benches);

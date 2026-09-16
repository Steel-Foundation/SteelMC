use super::benchmark_support::prepared_full_chunk;
use super::*;
use crate::behavior::init_behaviors;
use crate::chunk::chunk_holder::TickingReadiness;
use crate::test_support::{fresh_test_world, insert_ready_full_chunk, test_world};
use steel_registry::init_vanilla_registry;
use text_components::TextComponent;

struct RecordingConnection {
    packets: Arc<SyncMutex<Vec<EncodedPacket>>>,
}

impl NetworkConnection for RecordingConnection {
    fn compression(&self) -> Option<CompressionInfo> {
        None
    }

    fn send_encoded(&self, packet: EncodedPacket) {
        self.packets.lock().push(packet);
    }

    fn send_encoded_bundle(&self, packets: Vec<EncodedPacket>) {
        self.packets.lock().extend(packets);
    }

    fn disconnect_with_reason(&self, _reason: TextComponent) {}

    fn tick(&self) {}

    fn latency(&self) -> i32 {
        0
    }

    fn close(&self) {}

    fn closed(&self) -> bool {
        false
    }
}

fn pacing_with_outstanding_batches(batch_count: u16) -> ChunkBatchPacing {
    ChunkBatchPacing {
        unacknowledged_batches: batch_count,
        max_unacknowledged_batches: MAX_UNACKNOWLEDGED_BATCHES,
        ..ChunkBatchPacing::default()
    }
}

fn encode_positions(positions: &[ChunkPos]) -> (PreparedBatch, Vec<EncodedChunk>) {
    init_vanilla_registry();
    init_behaviors();
    let batch = PreparedBatch {
        chunks: positions.iter().copied().map(prepared_full_chunk).collect(),
        has_skylight: true,
        epoch_snapshot: 0,
    };
    let encoding_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .expect("test chunk encoding pool should initialize");
    let mut cache = FxHashMap::default();
    let encoded = ChunkSender::encode_batch(&batch, &mut cache, None, &encoding_pool);
    (batch, encoded)
}

#[test]
fn batch_resolution_keeps_every_still_pending_chunk_in_batch_order() {
    let positions = [
        ChunkPos::new(3, -2),
        ChunkPos::new(-1, 4),
        ChunkPos::new(8, 5),
        ChunkPos::new(0, 0),
    ];
    let (batch, encoded) = encode_positions(&positions);
    let pending = positions.iter().copied().collect::<FxHashSet<_>>();

    let valid = ChunkSender::resolve_valid_chunks(&batch.chunks, encoded, &pending);

    assert_eq!(
        valid.iter().map(|chunk| chunk.pos).collect::<Vec<_>>(),
        positions
    );
}

#[test]
fn batch_resolution_walks_past_prepared_chunks_that_failed_to_encode() {
    let positions = [
        ChunkPos::new(0, 0),
        ChunkPos::new(1, 0),
        ChunkPos::new(2, 0),
        ChunkPos::new(3, 0),
        ChunkPos::new(4, 0),
    ];
    let (batch, encoded) = encode_positions(&positions);
    let kept = [ChunkPos::new(1, 0), ChunkPos::new(4, 0)];
    let encoded = encoded
        .into_iter()
        .filter(|chunk| kept.contains(&chunk.pos))
        .collect::<Vec<_>>();
    let pending = positions.iter().copied().collect::<FxHashSet<_>>();

    let valid = ChunkSender::resolve_valid_chunks(&batch.chunks, encoded, &pending);

    assert_eq!(
        valid.iter().map(|chunk| chunk.pos).collect::<Vec<_>>(),
        kept
    );
}

#[test]
fn batch_resolution_drops_chunks_no_longer_pending() {
    let positions = [
        ChunkPos::new(0, 0),
        ChunkPos::new(1, 0),
        ChunkPos::new(2, 0),
    ];
    let (batch, encoded) = encode_positions(&positions);
    let mut pending = positions.iter().copied().collect::<FxHashSet<_>>();
    pending.remove(&ChunkPos::new(1, 0));

    let valid = ChunkSender::resolve_valid_chunks(&batch.chunks, encoded, &pending);

    assert_eq!(
        valid.iter().map(|chunk| chunk.pos).collect::<Vec<_>>(),
        [ChunkPos::new(0, 0), ChunkPos::new(2, 0)]
    );
}

#[test]
fn batch_resolution_drops_chunks_demoted_after_encoding() {
    let positions = [
        ChunkPos::new(0, 0),
        ChunkPos::new(1, 0),
        ChunkPos::new(2, 0),
    ];
    let (batch, encoded) = encode_positions(&positions);
    batch.chunks[1]
        .holder
        .transition_ticking_readiness(TickingReadiness::Unready);
    let pending = positions.iter().copied().collect::<FxHashSet<_>>();

    let valid = ChunkSender::resolve_valid_chunks(&batch.chunks, encoded, &pending);

    assert_eq!(
        valid.iter().map(|chunk| chunk.pos).collect::<Vec<_>>(),
        [ChunkPos::new(0, 0), ChunkPos::new(2, 0)]
    );
}

#[test]
fn parallel_chunk_encoding_preserves_batch_order_and_cache_entries() {
    init_vanilla_registry();
    init_behaviors();
    let positions = [
        ChunkPos::new(3, -2),
        ChunkPos::new(-1, 4),
        ChunkPos::new(8, 5),
        ChunkPos::new(0, 0),
    ];
    let batch = PreparedBatch {
        chunks: positions.into_iter().map(prepared_full_chunk).collect(),
        has_skylight: true,
        epoch_snapshot: 0,
    };
    let encoding_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .expect("test chunk encoding pool should initialize");
    let mut cache = FxHashMap::default();

    let encoded = ChunkSender::encode_batch(&batch, &mut cache, None, &encoding_pool);

    assert_eq!(
        encoded.iter().map(|chunk| chunk.pos).collect::<Vec<_>>(),
        positions
    );
    assert_eq!(cache.len(), positions.len());
    for chunk in &encoded {
        let cached = cache
            .get(&chunk.pos)
            .expect("every encoded chunk should be cached");
        assert!(Arc::ptr_eq(
            &cached.packet.encoded_data,
            &chunk.packet.encoded_data
        ));
    }

    let encoded_again = ChunkSender::encode_batch(&batch, &mut cache, None, &encoding_pool);
    for (first, second) in encoded.iter().zip(&encoded_again) {
        assert_eq!(first.pos, second.pos);
        assert!(Arc::ptr_eq(
            &first.packet.encoded_data,
            &second.packet.encoded_data
        ));
    }
}

#[test]
fn readiness_demotion_invalidates_prepared_chunk_encoding() {
    init_vanilla_registry();
    init_behaviors();
    let prepared = prepared_full_chunk(ChunkPos::new(4, -7));
    prepared
        .holder
        .transition_ticking_readiness(TickingReadiness::Unready);
    let batch = PreparedBatch {
        chunks: vec![prepared],
        has_skylight: true,
        epoch_snapshot: 0,
    };
    let encoding_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("test chunk encoding pool should initialize");
    let mut cache = FxHashMap::default();

    assert!(ChunkSender::encode_batch(&batch, &mut cache, None, &encoding_pool).is_empty());
    assert!(cache.is_empty());
}

#[test]
fn encoding_cache_requires_holder_identity_and_exact_readiness_generation() {
    init_vanilla_registry();
    init_behaviors();
    let pos = ChunkPos::new(-5, 9);
    let first_batch = PreparedBatch {
        chunks: vec![prepared_full_chunk(pos)],
        has_skylight: true,
        epoch_snapshot: 0,
    };
    let encoding_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("test chunk encoding pool should initialize");
    let mut cache = FxHashMap::default();

    let first = ChunkSender::encode_batch(&first_batch, &mut cache, None, &encoding_pool);
    assert_eq!(first.len(), 1);

    let replacement_batch = PreparedBatch {
        chunks: vec![prepared_full_chunk(pos)],
        has_skylight: true,
        epoch_snapshot: 0,
    };
    let replacement =
        ChunkSender::encode_batch(&replacement_batch, &mut cache, None, &encoding_pool);
    assert_eq!(replacement.len(), 1);
    assert!(!Arc::ptr_eq(
        &first[0].packet.encoded_data,
        &replacement[0].packet.encoded_data
    ));

    let holder = Arc::clone(&replacement_batch.chunks[0].holder);
    holder.transition_ticking_readiness(TickingReadiness::Unready);
    holder.transition_ticking_readiness(TickingReadiness::BlockTicking);
    let rebound_batch = PreparedBatch {
        chunks: vec![PreparedChunk {
            pos,
            readiness: holder.ticking_readiness_snapshot(),
            holder,
        }],
        has_skylight: true,
        epoch_snapshot: 0,
    };
    let rebound = ChunkSender::encode_batch(&rebound_batch, &mut cache, None, &encoding_pool);
    assert_eq!(rebound.len(), 1);
    assert!(!Arc::ptr_eq(
        &replacement[0].packet.encoded_data,
        &rebound[0].packet.encoded_data
    ));
}

#[test]
fn chunk_batch_ack_without_outstanding_batch_does_not_update_pacing() {
    let mut sender = ChunkSender::default();

    assert!(!sender.on_chunk_batch_received_by_client(64.0));
    assert_eq!(sender.pacing.unacknowledged_batches, 0);
    assert_eq!(
        sender.pacing.desired_chunks_per_tick.to_bits(),
        START_CHUNKS_PER_TICK.to_bits()
    );
    assert_eq!(sender.pacing.batch_quota.to_bits(), 0.0_f32.to_bits());
    assert_eq!(sender.pacing.max_unacknowledged_batches, 1);
    assert!(sender.pacing.accepted_feedback.is_empty());
}

#[test]
fn chunk_batch_ack_updates_pacing_at_the_next_prepare_boundary() {
    let mut sender = ChunkSender::default();
    sender.pacing.unacknowledged_batches = 1;

    assert!(sender.on_chunk_batch_received_by_client(f32::NAN));
    assert_eq!(sender.pacing.unacknowledged_batches, 1);
    assert_eq!(
        sender.pacing.desired_chunks_per_tick.to_bits(),
        START_CHUNKS_PER_TICK.to_bits()
    );
    assert_eq!(sender.pacing.batch_quota.to_bits(), 0.0_f32.to_bits());

    let epoch = SyncMutex::new(0);
    assert!(
        sender
            .prepare_batch(test_world(), ChunkPos::new(0, 0), &epoch)
            .is_none()
    );
    assert_eq!(sender.pacing.unacknowledged_batches, 0);
    assert_eq!(
        sender.pacing.desired_chunks_per_tick.to_bits(),
        MIN_CHUNKS_PER_TICK.to_bits()
    );
    assert_eq!(sender.pacing.batch_quota.to_bits(), 1.0_f32.to_bits());
    assert_eq!(
        sender.pacing.max_unacknowledged_batches,
        MAX_UNACKNOWLEDGED_BATCHES
    );
    assert!(sender.pacing.accepted_feedback.is_empty());
}

#[test]
fn ack_between_prepare_and_commit_cannot_overwrite_the_current_batch() {
    init_vanilla_registry();
    init_behaviors();
    let world = fresh_test_world("chunk_batch_ack_prepare_commit_race");
    let positions = [ChunkPos::new(0, 0), ChunkPos::new(1, 0)];
    for pos in positions {
        insert_ready_full_chunk(&world, pos);
    }

    let mut sender = ChunkSender {
        pacing: ChunkBatchPacing {
            unacknowledged_batches: 1,
            desired_chunks_per_tick: 2.0,
            batch_quota: 0.0,
            max_unacknowledged_batches: MAX_UNACKNOWLEDGED_BATCHES,
            accepted_feedback: SmallVec::new(),
        },
        ..ChunkSender::default()
    };
    sender.pending_chunks.extend(positions);
    let epoch = SyncMutex::new(0);
    let batch = sender
        .prepare_batch(&world, positions[0], &epoch)
        .expect("two ready chunks should prepare");
    assert_eq!(batch.chunks.len(), 2);

    let encoding_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("test chunk encoding pool should initialize");
    let mut cache = FxHashMap::default();
    let encoded = ChunkSender::encode_batch(&batch, &mut cache, None, &encoding_pool);
    assert_eq!(encoded.len(), 2);

    assert!(sender.on_chunk_batch_received_by_client(0.5));
    assert_eq!(sender.pacing.unacknowledged_batches, 1);
    assert_eq!(sender.pacing.batch_quota.to_bits(), 2.0_f32.to_bits());

    let packets = Arc::new(SyncMutex::new(Vec::new()));
    let connection = PlayerConnection::Other(Box::new(RecordingConnection {
        packets: Arc::clone(&packets),
    }));
    let sent = sender.commit_batch(&batch, encoded, &connection, &epoch);

    assert_eq!(sent.len(), 2);
    assert_eq!(packets.lock().len(), 4);
    assert!(sender.pending_chunks.is_empty());
    for pos in positions {
        assert!(sender.is_chunk_sent(pos));
    }
    assert_eq!(sender.pacing.unacknowledged_batches, 2);
    assert_eq!(sender.pacing.batch_quota.to_bits(), 0.0_f32.to_bits());

    assert!(sender.prepare_batch(&world, positions[0], &epoch).is_none());
    assert_eq!(sender.pacing.unacknowledged_batches, 1);
    assert_eq!(
        sender.pacing.desired_chunks_per_tick.to_bits(),
        0.5_f32.to_bits()
    );
    assert_eq!(sender.pacing.batch_quota.to_bits(), 0.5_f32.to_bits());
}

#[test]
fn ack_mailbox_is_bounded_by_one_and_ten_outstanding_batches() {
    let mut one_outstanding = pacing_with_outstanding_batches(1);
    assert!(one_outstanding.record_feedback(1.0));
    assert!(!one_outstanding.record_feedback(2.0));
    assert_eq!(one_outstanding.accepted_feedback.as_slice(), &[1.0]);

    let mut ten_outstanding = pacing_with_outstanding_batches(10);
    for desired_chunks_per_tick in 1..=10 {
        assert!(ten_outstanding.record_feedback(desired_chunks_per_tick as f32));
    }
    assert!(!ten_outstanding.record_feedback(11.0));
    assert_eq!(ten_outstanding.accepted_feedback.len(), 10);
    assert!(!ten_outstanding.accepted_feedback.spilled());
}

#[test]
fn feedback_drains_before_the_outstanding_batch_gate() {
    let mut pacing = ChunkBatchPacing {
        unacknowledged_batches: 1,
        desired_chunks_per_tick: START_CHUNKS_PER_TICK,
        batch_quota: 0.0,
        max_unacknowledged_batches: 1,
        accepted_feedback: SmallVec::from_slice(&[2.0]),
    };

    assert_eq!(pacing.begin_prepare(), Some(2));
    assert_eq!(pacing.unacknowledged_batches, 0);
    assert_eq!(
        pacing.max_unacknowledged_batches,
        MAX_UNACKNOWLEDGED_BATCHES
    );
    assert_eq!(pacing.batch_quota.to_bits(), 2.0_f32.to_bits());
}

#[test]
fn feedback_drains_in_arrival_order_and_handles_nan() {
    let mut pacing = pacing_with_outstanding_batches(3);
    assert!(pacing.record_feedback(MAX_CHUNKS_PER_TICK + 1.0));
    assert!(pacing.record_feedback(2.5));
    assert!(pacing.record_feedback(f32::NAN));

    assert_eq!(pacing.begin_prepare(), Some(1));
    assert_eq!(pacing.unacknowledged_batches, 0);
    assert_eq!(
        pacing.desired_chunks_per_tick.to_bits(),
        MIN_CHUNKS_PER_TICK.to_bits()
    );
    assert_eq!(pacing.batch_quota.to_bits(), 1.0_f32.to_bits());
    assert!(pacing.accepted_feedback.is_empty());
}

#[test]
fn first_ack_expands_the_send_window_and_full_window_resumes_after_feedback() {
    let mut pacing = ChunkBatchPacing::default();
    assert_eq!(pacing.begin_prepare(), Some(9));
    pacing.commit_batch(1);
    assert_eq!(pacing.begin_prepare(), None);

    assert!(pacing.record_feedback(2.0));
    for _ in 0..10 {
        assert_eq!(pacing.begin_prepare(), Some(2));
        pacing.commit_batch(2);
    }
    assert_eq!(pacing.begin_prepare(), None);

    assert!(pacing.record_feedback(2.0));
    assert_eq!(pacing.begin_prepare(), Some(2));
    pacing.commit_batch(2);
    assert_eq!(pacing.begin_prepare(), None);
}

#[test]
fn fractional_feedback_accumulates_credit_until_a_whole_chunk_can_be_sent() {
    let mut pacing = pacing_with_outstanding_batches(2);
    assert!(pacing.record_feedback(0.25));

    for _ in 0..2 {
        for _ in 0..3 {
            assert_eq!(pacing.begin_prepare(), Some(0));
        }
        assert_eq!(pacing.begin_prepare(), Some(1));
        pacing.commit_batch(1);
        assert_eq!(pacing.batch_quota.to_bits(), 0.0_f32.to_bits());
    }
}

#[test]
fn feedback_clamps_nonfinite_and_out_of_range_rates_before_preparing() {
    for (feedback, expected_rate, expected_quota) in [
        (f32::NAN, MIN_CHUNKS_PER_TICK, 0),
        (f32::NEG_INFINITY, MIN_CHUNKS_PER_TICK, 0),
        (-1.0, MIN_CHUNKS_PER_TICK, 0),
        (0.0, MIN_CHUNKS_PER_TICK, 0),
        (2.5, 2.5, 2),
        (MAX_CHUNKS_PER_TICK + 1.0, MAX_CHUNKS_PER_TICK, 500),
        (f32::INFINITY, MAX_CHUNKS_PER_TICK, 500),
    ] {
        let mut pacing = pacing_with_outstanding_batches(2);
        assert!(pacing.record_feedback(feedback));
        assert_eq!(pacing.begin_prepare(), Some(expected_quota));
        assert_eq!(
            pacing.desired_chunks_per_tick.to_bits(),
            expected_rate.to_bits()
        );
    }
}

#[test]
fn filtered_commit_charges_only_sent_chunks_and_keeps_feedback_until_next_prepare() {
    for retained in 0..=2 {
        let positions = [ChunkPos::new(0, 0), ChunkPos::new(1, 0)];
        let (batch, encoded) = encode_positions(&positions);
        let mut sender = ChunkSender {
            pacing: ChunkBatchPacing {
                desired_chunks_per_tick: 2.0,
                ..pacing_with_outstanding_batches(1)
            },
            ..ChunkSender::default()
        };
        sender.pending_chunks.extend(positions);
        assert_eq!(sender.pacing.begin_prepare(), Some(2));
        assert!(sender.on_chunk_batch_received_by_client(0.5));
        for prepared in &batch.chunks[retained..] {
            prepared
                .holder
                .transition_ticking_readiness(TickingReadiness::Unready);
        }

        let packets = Arc::new(SyncMutex::new(Vec::new()));
        let connection = PlayerConnection::Other(Box::new(RecordingConnection {
            packets: Arc::clone(&packets),
        }));
        let sent = sender.commit_batch(&batch, encoded, &connection, &SyncMutex::new(0));

        assert_eq!(sent, positions[..retained]);
        assert_eq!(
            sender.pacing.unacknowledged_batches,
            if retained == 0 { 1 } else { 2 }
        );
        assert_eq!(
            sender.pacing.batch_quota.to_bits(),
            (2.0 - retained as f32).to_bits()
        );
        assert_eq!(sender.pacing.accepted_feedback.as_slice(), &[0.5]);
        assert_eq!(
            packets.lock().len(),
            if retained == 0 { 0 } else { retained + 2 }
        );

        assert_eq!(
            sender.pacing.begin_prepare(),
            Some(usize::from(retained < 2))
        );
        assert_eq!(
            sender.pacing.unacknowledged_batches,
            u16::from(retained != 0)
        );
    }
}

#[test]
fn invalidated_epoch_discards_batch_without_consuming_quota_or_accepted_feedback() {
    let positions = [ChunkPos::new(0, 0), ChunkPos::new(1, 0)];
    let (batch, encoded) = encode_positions(&positions);
    let mut sender = ChunkSender {
        pacing: ChunkBatchPacing {
            desired_chunks_per_tick: 2.0,
            ..pacing_with_outstanding_batches(1)
        },
        ..ChunkSender::default()
    };
    sender.pending_chunks.extend(positions);
    assert_eq!(sender.pacing.begin_prepare(), Some(2));
    assert!(sender.on_chunk_batch_received_by_client(0.5));

    let packets = Arc::new(SyncMutex::new(Vec::new()));
    let connection = PlayerConnection::Other(Box::new(RecordingConnection {
        packets: Arc::clone(&packets),
    }));
    assert_eq!(
        sender.commit_batch(&batch, encoded, &connection, &SyncMutex::new(1)),
        Vec::<ChunkPos>::new()
    );
    assert!(packets.lock().is_empty());
    assert_eq!(sender.pending_chunks.len(), 2);
    assert_eq!(sender.pacing.unacknowledged_batches, 1);
    assert_eq!(sender.pacing.batch_quota.to_bits(), 2.0_f32.to_bits());
    assert_eq!(sender.pacing.accepted_feedback.as_slice(), &[0.5]);
    assert_eq!(sender.pacing.begin_prepare(), Some(1));
    assert_eq!(sender.pacing.unacknowledged_batches, 0);
}

#[test]
fn clearing_tracking_keeps_old_batch_ack_separate_from_new_world_batch() {
    let mut sender = ChunkSender {
        pacing: ChunkBatchPacing {
            unacknowledged_batches: 1,
            desired_chunks_per_tick: 2.0,
            batch_quota: 1.0,
            max_unacknowledged_batches: MAX_UNACKNOWLEDGED_BATCHES,
            accepted_feedback: SmallVec::new(),
        },
        ..ChunkSender::default()
    };
    sender.pending_chunks.insert(ChunkPos::new(1, 2));
    sender.mark_chunk_sent_for_test(ChunkPos::new(3, 4));

    sender.clear_world_chunks();

    assert!(sender.pending_chunks.is_empty());
    assert!(sender.sent_chunks.is_empty());

    sender.pacing.commit_batch(1);
    assert_eq!(sender.pacing.unacknowledged_batches, 2);
    assert!(sender.on_chunk_batch_received_by_client(20.0));

    assert_eq!(sender.pacing.begin_prepare(), Some(20));
    assert_eq!(sender.pacing.unacknowledged_batches, 1);
}

#[test]
fn clearing_world_chunks_preserves_connection_pacing() {
    let pending = ChunkPos::new(2, -3);
    let sent = ChunkPos::new(-5, 7);
    let mut sender = ChunkSender {
        pacing: ChunkBatchPacing {
            unacknowledged_batches: 3,
            desired_chunks_per_tick: 12.5,
            batch_quota: 4.5,
            max_unacknowledged_batches: MAX_UNACKNOWLEDGED_BATCHES,
            accepted_feedback: SmallVec::new(),
        },
        ..ChunkSender::default()
    };
    sender.mark_chunk_pending_to_send(pending);
    sender.mark_chunk_sent_for_test(sent);

    sender.clear_world_chunks();

    assert!(sender.pending_chunks.is_empty());
    assert!(sender.sent_chunks.is_empty());
    assert_eq!(sender.pacing.unacknowledged_batches, 3);
    assert_eq!(
        sender.pacing.desired_chunks_per_tick.to_bits(),
        12.5_f32.to_bits()
    );
    assert_eq!(sender.pacing.batch_quota.to_bits(), 4.5_f32.to_bits());
    assert_eq!(
        sender.pacing.max_unacknowledged_batches,
        MAX_UNACKNOWLEDGED_BATCHES
    );
}

#[test]
fn marking_chunk_pending_clears_sent_state() {
    let mut sender = ChunkSender::default();
    let pos = ChunkPos::new(2, -3);
    sender.sent_chunks.insert(pos);

    sender.mark_chunk_pending_to_send(pos);

    assert!(sender.pending_chunks.contains(&pos));
    assert!(!sender.is_chunk_sent(pos));
}

#[test]
fn chunk_distance_squared_handles_far_chunk_coordinates() {
    let distance = ChunkSender::chunk_distance_squared(
        ChunkPos::new(1_250_000, -1_250_000),
        ChunkPos::new(0, 0),
    );

    assert_eq!(distance, 3_125_000_000_000);
}

#[test]
fn chunk_distance_squared_handles_valid_world_extremes() {
    let max = ChunkPos::MAX_COORDINATE_VALUE;
    let delta = u64::from(max.abs_diff(-max));
    let expected = delta * delta * 2;

    let distance =
        ChunkSender::chunk_distance_squared(ChunkPos::new(max, max), ChunkPos::new(-max, -max));

    assert_eq!(distance, expected);
}

#[test]
fn chunk_distance_squared_saturates_for_invalid_i32_extremes() {
    let distance = ChunkSender::chunk_distance_squared(
        ChunkPos::new(i32::MIN, i32::MIN),
        ChunkPos::new(i32::MAX, i32::MAX),
    );

    assert_eq!(distance, u64::MAX);
}

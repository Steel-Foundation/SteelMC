use super::*;

#[test]
fn duplicate_positions_are_not_inserted_twice() {
    let mut set = JavaBlockPosSet::default();
    assert!(set.insert(BlockPos::new(1, 2, 3)));
    assert!(!set.insert(BlockPos::new(1, 2, 3)));
    assert_eq!(set.java_ordered_positions(), [BlockPos::new(1, 2, 3)]);
}

#[test]
fn iteration_matches_java_hash_set_after_resize() {
    let mut set = JavaBlockPosSet::default();
    for x in 0..20 {
        assert!(set.insert(BlockPos::new(x, 64, x % 5)));
    }

    assert_eq!(
        set.java_ordered_positions(),
        [
            (0, 0),
            (1, 1),
            (2, 2),
            (5, 0),
            (3, 3),
            (6, 1),
            (4, 4),
            (7, 2),
            (10, 0),
            (8, 3),
            (11, 1),
            (9, 4),
            (12, 2),
            (15, 0),
            (13, 3),
            (16, 1),
            (14, 4),
            (17, 2),
            (18, 3),
            (19, 4),
        ]
        .map(|(x, z)| BlockPos::new(x, 64, z))
    );
}

#[test]
fn collision_chain_triggers_java_hash_map_resize() {
    let mut set = JavaBlockPosSet::default();
    for x in (0..=128).step_by(16) {
        assert!(set.insert(BlockPos::new(x, 0, 0)));
    }

    assert_eq!(
        set.java_ordered_positions(),
        [0, 32, 64, 96, 128, 16, 48, 80, 112].map(|x| BlockPos::new(x, 0, 0))
    );
}

#[test]
fn reinserted_position_returns_to_its_bucket_tail() {
    let mut set = JavaBlockPosSet::default();
    let first = BlockPos::new(0, 0, 0);
    let second = BlockPos::new(16, 0, 0);

    assert!(set.insert(first));
    assert!(set.insert(second));
    assert_eq!(set.pop_java_ordered_position(), Some(first));
    assert!(set.insert(first));

    assert_eq!(set.java_ordered_positions(), [second, first]);
}

#[test]
fn pop_uses_java_bucket_order() {
    let mut set = JavaBlockPosSet::default();
    let first = BlockPos::new(15, 0, 0);
    let second = BlockPos::new(1, 0, 0);
    assert!(set.insert(first));
    assert!(set.insert(second));

    assert_eq!(set.pop_java_ordered_position(), Some(second));
    assert_eq!(set.pop_java_ordered_position(), Some(first));
    assert_eq!(set.pop_java_ordered_position(), None);
}

#[test]
fn tree_bin_insertion_and_iterator_removal_match_java_hash_set() {
    let mut set = JavaBlockPosSet::default();
    for i in [
        17, 3, 29, 1, 11, 23, 7, 31, 5, 19, 13, 2, 37, 0, 41, 9, 27, 15, 35, 21, 6, 33, 25, 39, 4,
        43, 8, 45, 10, 47,
    ] {
        assert!(set.insert(BlockPos::new(i * 64, 0, 0)));
    }

    assert_eq!(
        set.java_order().map(BlockPos::x).collect::<Vec<_>>(),
        [
            1088, 192, 1856, 64, 0, 128, 704, 576, 640, 512, 1472, 1728, 1600, 448, 1984, 2240,
            2112, 2368, 2624, 2752, 2880, 3008, 2496, 320, 256, 384, 1216, 1344, 832, 960,
        ]
    );

    for expected in [1088, 192, 1856, 64, 0] {
        assert_eq!(
            set.pop_java_ordered_position(),
            Some(BlockPos::new(expected, 0, 0))
        );
    }
    assert!(set.insert(BlockPos::new(1088, 0, 0)));
    for i in [49, 51, 53, 55, 57] {
        assert!(set.insert(BlockPos::new(i * 64, 0, 0)));
    }

    assert_eq!(
        set.java_order().map(BlockPos::x).collect::<Vec<_>>(),
        [
            1984, 1216, 128, 704, 576, 640, 512, 1472, 1728, 1600, 448, 2240, 2112, 2368, 2624,
            2752, 2880, 3008, 3136, 3264, 3392, 3520, 3648, 2496, 320, 256, 384, 1344, 832, 960,
            1088,
        ]
    );
}

#[test]
fn tree_bin_resize_matches_java_hash_set() {
    let mut set = JavaBlockPosSet::default();
    for i in [
        34, 23, 3, 8, 44, 25, 11, 52, 36, 10, 21, 18, 20, 9, 54, 41, 40, 42, 12, 16, 14, 47, 35, 6,
        46, 31, 53, 5, 13, 7, 51, 29, 33, 0, 24, 32, 22, 1, 27, 37, 30, 50, 48, 43, 4, 15, 26, 2,
        45, 28, 39, 38, 17, 49, 19, 55,
    ] {
        assert!(set.insert(BlockPos::new(i * 64, 0, 0)));
    }

    assert_eq!(
        set.java_order().map(BlockPos::x).collect::<Vec<_>>(),
        [
            1408, 2176, 128, 256, 0, 384, 512, 2816, 1664, 1536, 1920, 1792, 2048, 3328, 3200,
            3072, 2944, 3456, 2304, 2688, 2560, 2432, 640, 1152, 768, 896, 1024, 1280, 1344, 448,
            1472, 192, 64, 320, 1600, 1728, 1984, 2112, 1856, 704, 3008, 3264, 3136, 2880, 3392,
            3520, 2368, 2240, 2624, 2496, 2752, 576, 960, 1088, 1216, 832,
        ]
    );
}

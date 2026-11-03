use std::collections::BTreeSet;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use super::*;

#[derive(serde::Deserialize)]
struct CollectionFixtures {
    minecraft_version: String,
    java_feature_version: u32,
    cases: Vec<CollectionCase>,
}

#[derive(serde::Deserialize)]
struct CollectionCase {
    name: String,
    insertions: Vec<CollectionInsertion>,
}

#[derive(serde::Deserialize)]
struct CollectionInsertion {
    position: [i32; 3],
    inserted: bool,
    capacity: usize,
    tree_bins: usize,
    order_sha256: String,
    tree_shape_sha256: String,
}

#[test]
fn linked_order_matches_openjdk_25_after_each_insertion_and_resize() {
    let fixtures: CollectionFixtures =
        serde_json::from_str(include_str!("../../test_assets/java_block_pos_sets.json"))
            .expect("SteelExtractor collection fixtures must be valid JSON");
    assert_eq!(fixtures.minecraft_version, steel_utils::MC_VERSION);
    assert_eq!(fixtures.java_feature_version, 25);
    let mut saw_tree = false;
    for case in fixtures.cases {
        saw_tree |= check_case(&case);
    }
    assert!(saw_tree, "the fixtures must exercise actual tree bins");
}

fn check_case(case: &CollectionCase) -> bool {
    let mut set = JavaBlockPosSet::new(EqualHashOrder::Insertion);
    let mut saw_tree = false;
    for (index, insertion) in case.insertions.iter().enumerate() {
        let [x, y, z] = insertion.position;
        let pos = BlockPos::new(x, y, z);
        let context = format!("{}: insertion {index} {pos:?}", case.name);
        assert_eq!(set.insert(pos), insertion.inserted, "{context}");
        assert_eq!(set.bucket_count(), insertion.capacity, "{context}");
        let trees = set.buckets.iter().filter(|bucket| bucket.is_tree()).count();
        assert_eq!(trees, insertion.tree_bins, "{context}");
        saw_tree |= trees != 0;
        check_links(&set);
        let mut digest = Sha256::new();
        for bucket in &set.buckets {
            let mut node = bucket.head;
            while node != EMPTY {
                let entry = &set.entries[node as usize];
                digest.update(format!(
                    "{} {} {}\n",
                    entry.pos.x(),
                    entry.pos.y(),
                    entry.pos.z()
                ));
                node = entry.next;
            }
        }
        let mut actual = String::new();
        for byte in digest.finalize() {
            write!(actual, "{byte:02x}").expect("string write");
        }
        assert_eq!(actual, insertion.order_sha256, "{context}");
        let mut shape = String::new();
        for (index, bucket) in set.buckets.iter().enumerate() {
            if bucket.is_tree() {
                write!(shape, "{index}:").expect("string write");
                tree_shape(&set, bucket.head, &mut shape);
            }
        }
        let mut actual_shape = String::new();
        for byte in Sha256::digest(shape.as_bytes()) {
            write!(actual_shape, "{byte:02x}").expect("string write");
        }
        assert_eq!(
            actual_shape, insertion.tree_shape_sha256,
            "{context} tree shape"
        );
    }
    saw_tree
}

fn check_links(set: &JavaBlockPosSet) {
    let mut all = BTreeSet::new();
    for bucket in &set.buckets {
        let mut list = BTreeSet::new();
        let mut node = bucket.head;
        let mut prev = EMPTY;
        while node != EMPTY {
            assert!(all.insert(node), "list cycle or repeated bucket entry");
            list.insert(node);
            if bucket.is_tree() {
                assert_eq!(set.links(node).prev, prev, "broken reverse iteration link");
            }
            prev = node;
            node = set.entries[node as usize].next;
        }
        if !bucket.is_tree() {
            continue;
        }
        let root = set.tree_root(bucket.head);
        assert_eq!(set.links(root).parent, EMPTY, "root has a parent");
        let mut tree = BTreeSet::new();
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            assert!(tree.insert(node), "tree cycle or repeated child");
            let links = set.links(node);
            for child in [links.left, links.right] {
                if child != EMPTY {
                    assert_eq!(set.links(child).parent, node, "broken parent link");
                    pending.push(child);
                }
            }
        }
        assert_eq!(
            tree, list,
            "tree and iteration list contain different entries"
        );
    }
    assert_eq!(all.len(), set.len, "unreachable set entry");
}

#[test]
fn removing_tree_heads_preserves_order_and_lookup_for_both_policies() {
    for policy in [EqualHashOrder::Coordinates, EqualHashOrder::Insertion] {
        let mut set = JavaBlockPosSet::new(policy);
        for z in (0..64).rev() {
            assert!(set.insert(BlockPos::new(-961 * z, 0, z)));
        }
        let positions = set.java_ordered_positions();
        assert!(set.buckets.iter().any(|bucket| bucket.is_tree()));
        for (index, &pos) in positions.iter().enumerate() {
            assert_eq!(set.pop_java_ordered_position(), Some(pos));
            assert!(!set.contains(pos));
            assert_eq!(set.java_ordered_positions(), positions[index + 1..]);
            for &remaining in &positions[index + 1..] {
                assert!(set.contains(remaining));
            }
            check_links(&set);
        }
        assert!(set.is_empty());
        assert_eq!(set.pop_java_ordered_position(), None);
        assert!(set.insert(positions[0]));
        assert_eq!(set.java_ordered_positions(), [positions[0]]);
        check_links(&set);
    }
}

#[test]
fn removed_entries_do_not_count_towards_the_resize_threshold() {
    let mut set = JavaBlockPosSet::default();
    for x in 0..12 {
        assert!(set.insert(BlockPos::new(x, 0, 0)));
    }
    let capacity = set.bucket_count();
    for _ in 0..11 {
        assert!(set.pop_java_ordered_position().is_some());
    }
    for x in 12..23 {
        assert!(set.insert(BlockPos::new(x, 0, 0)));
    }
    assert_eq!(set.bucket_count(), capacity);
    check_links(&set);
}

#[test]
fn resize_untreeifies_small_unsplit_bins_after_iterator_removals() {
    for policy in [EqualHashOrder::Coordinates, EqualHashOrder::Insertion] {
        let mut set = JavaBlockPosSet::new(policy);
        for x in 0..12 {
            assert!(set.insert(BlockPos::new(x * 128, 0, 0)));
        }
        assert!(set.buckets.iter().any(|bucket| bucket.is_tree()));
        for _ in 0..6 {
            assert!(set.pop_java_ordered_position().is_some());
        }
        let positions = set.java_ordered_positions();
        assert!(set.buckets.iter().any(|bucket| bucket.is_tree()));

        set.resize();

        assert!(!set.buckets.iter().any(|bucket| bucket.is_tree()));
        assert_eq!(set.java_ordered_positions(), positions);
        for pos in positions {
            assert!(set.contains(pos));
        }
        check_links(&set);
    }
}

fn tree_shape(set: &JavaBlockPosSet, node: u32, out: &mut String) {
    if node == EMPTY {
        out.push('.');
        return;
    }
    let pos = set.entries[node as usize].pos;
    let links = set.links(node);
    write!(
        out,
        "({},{},{},{}",
        pos.x(),
        pos.y(),
        pos.z(),
        if links.red { 'r' } else { 'b' }
    )
    .expect("string write");
    tree_shape(set, links.left, out);
    tree_shape(set, links.right, out);
    out.push(')');
}

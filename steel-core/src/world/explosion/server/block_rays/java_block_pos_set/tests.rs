use std::collections::BTreeSet;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use super::*;

#[test]
fn linked_order_matches_openjdk_25_after_each_insertion_and_resize() {
    check_oracle(include_str!("tests/openjdk25.txt"));
    check_oracle(include_str!("tests/openjdk25-sequential-identity.txt"));
}

fn check_oracle(oracle: &str) {
    let mut set = JavaBlockPosSet::default();
    let mut case = "";
    let mut saw_tree = false;
    for line in oracle.lines() {
        if let Some(name) = line.strip_prefix("case ") {
            case = name;
            set = JavaBlockPosSet::default();
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let pos = BlockPos::new(
            fields[0].parse().expect("oracle coordinate"),
            fields[1].parse().expect("oracle coordinate"),
            fields[2].parse().expect("oracle coordinate"),
        );
        assert_eq!(set.insert(pos), fields[3] == "true", "{case}: {pos:?}");
        assert_eq!(
            set.bucket_count(),
            fields[4].parse::<usize>().expect("oracle count"),
            "{case}: {pos:?}"
        );
        let trees = set.buckets.iter().filter(|bucket| bucket.is_tree()).count();
        assert_eq!(
            trees,
            fields[5].parse::<usize>().expect("oracle count"),
            "{case}: {pos:?}"
        );
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
        assert_eq!(actual, fields[6], "{case}: {pos:?}");
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
        assert_eq!(actual_shape, fields[7], "{case}: {pos:?} tree shape");
    }
    assert!(saw_tree, "the oracle must exercise actual tree bins");
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
        assert_eq!(set.links(bucket.head).parent, EMPTY, "head is not the root");
        let mut tree = BTreeSet::new();
        let mut pending = vec![bucket.head];
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
    assert_eq!(all.len(), set.entries.len(), "unreachable set entry");
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

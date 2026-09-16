use crate::advancement::registry::AdvancementRegistry;

/// calculate the positions of advancement nodes using the Reingold-Tilford algorithm the same used by minecraft.
///
/// the resulting x position are random so can't really be compared to vanilla
pub fn run(tree: &mut AdvancementRegistry, root_index: usize) {
    let Some(root_node) = tree.adv_nodes.get(root_index) else {
        eprintln!("AdvancementNode index out of bounds");
        return;
    };
    if !root_node.has_display() {
        eprintln!("Can't position children of an invisible root!");
        return;
    }
    //store everything inside a vector to not have to deal with pointer
    let mut nodes: Vec<TreeNodePosition> = Vec::with_capacity(32);
    let root_idx = nodes.len();
    nodes.push(TreeNodePosition {
        node: root_index,
        parent: None,
        previous_sibling: None,
        child_index: 1,
        children: Vec::new(),
        ancestor: root_idx,
        thread: None,
        x: 0,
        y: -1.0,
        mod_field: 0.0,
        change: 0.0,
        shift: 0.0,
    });

    let mut previous_idx = None;
    for child in root_node.children.clone() {
        previous_idx = TreeNodePosition::add_child(&mut nodes, tree, root_idx, child, previous_idx);
    }

    TreeNodePosition::first_walk(&mut nodes, root_idx);

    let root_y = nodes[root_idx].y;
    let min = TreeNodePosition::second_walk(&mut nodes, root_idx, 0.0, 0, root_y);

    if min < 0.0 {
        TreeNodePosition::third_walk(&mut nodes, -min);
    }

    TreeNodePosition::finalize_position(tree, &nodes, root_idx);
}

struct TreeNodePosition {
    node: usize,
    parent: Option<usize>,
    previous_sibling: Option<usize>,
    child_index: usize,
    children: Vec<usize>,
    ancestor: usize,
    thread: Option<usize>,
    x: i32,
    y: f32,
    mod_field: f32,
    change: f32,
    shift: f32,
}

impl TreeNodePosition {
    fn add_child(
        nodes: &mut Vec<TreeNodePosition>,
        tree: &mut AdvancementRegistry,
        parent_idx: usize,
        adv_node_idx: usize,
        mut previous_idx: Option<usize>,
    ) -> Option<usize> {
        let adv_node = tree.adv_nodes.get(adv_node_idx)?;
        if adv_node.has_display() {
            let child_idx = nodes.len();
            let node = &mut nodes[parent_idx];
            let next_child_index = node.children.len() + 1;
            let depth = node.x + 1;
            node.children.push(child_idx);

            nodes.push(TreeNodePosition {
                node: adv_node_idx,
                parent: Some(parent_idx),
                previous_sibling: previous_idx,
                child_index: next_child_index,
                children: Vec::new(),
                ancestor: child_idx,
                thread: None,
                x: depth,
                y: -1.0,
                mod_field: 0.0,
                change: 0.0,
                shift: 0.0,
            });

            let mut child_prev = None;
            for child in adv_node.children.clone() {
                child_prev = Self::add_child(nodes, tree, child_idx, child, child_prev);
            }

            Some(child_idx)
        } else {
            for grandchild in &adv_node.children.clone() {
                previous_idx = Self::add_child(nodes, tree, parent_idx, *grandchild, previous_idx);
            }
            previous_idx
        }
    }

    fn first_walk(nodes: &mut Vec<TreeNodePosition>, idx: usize) {
        let num_children = nodes[idx].children.len();
        if num_children == 0 {
            if let Some(prev_sib) = nodes[idx].previous_sibling {
                nodes[idx].y = nodes[prev_sib].y + 1.0;
            } else {
                nodes[idx].y = 0.0;
            }
        } else {
            let mut default_ancestor: Option<usize> = None;
            for i in 0..num_children {
                let child_idx = nodes[idx].children[i];
                Self::first_walk(nodes, child_idx);
                let arg_ancestor = default_ancestor.unwrap_or(child_idx);
                default_ancestor = Some(Self::apportion(nodes, child_idx, arg_ancestor));
            }

            Self::execute_shifts(nodes, idx);

            let node = &mut nodes[idx];
            let first_child_idx = node.children[0];
            let last_child_idx = node.children[num_children - 1];
            let midpoint = f32::midpoint(nodes[first_child_idx].y, nodes[last_child_idx].y);

            if let Some(prev_sib) = nodes[idx].previous_sibling {
                nodes[idx].y = nodes[prev_sib].y + 1.0;
                nodes[idx].mod_field = nodes[idx].y - midpoint;
            } else {
                nodes[idx].y = midpoint;
            }
        }
    }

    fn second_walk(
        nodes: &mut Vec<TreeNodePosition>,
        idx: usize,
        mod_sum: f32,
        depth: i32,
        mut min: f32,
    ) -> f32 {
        let node = &mut nodes[idx];
        node.y += mod_sum;
        node.x = depth;

        if node.y < min {
            min = node.y;
        }

        let num_children = node.children.len();
        let current_mod = node.mod_field;

        for i in 0..num_children {
            let child_idx = nodes[idx].children[i];
            min = Self::second_walk(nodes, child_idx, mod_sum + current_mod, depth + 1, min);
        }

        min
    }

    fn third_walk(nodes: &mut [TreeNodePosition], offset: f32) {
        for node in nodes.iter_mut() {
            node.y += offset;
        }
    }

    fn execute_shifts(nodes: &mut [TreeNodePosition], idx: usize) {
        let mut shift = 0.0;
        let mut change = 0.0;

        for &child_idx in nodes[idx].children.iter().rev() {
            nodes[child_idx].y += shift;
            nodes[child_idx].mod_field += shift;
            change += nodes[child_idx].change;
            shift += nodes[child_idx].shift + change;
        }
    }

    #[inline]
    fn previous_or_thread(nodes: &[TreeNodePosition], idx: usize) -> Option<usize> {
        nodes[idx]
            .thread
            .or_else(|| nodes[idx].children.first().copied())
    }

    #[inline]
    fn next_or_thread(nodes: &[TreeNodePosition], idx: usize) -> Option<usize> {
        nodes[idx]
            .thread
            .or_else(|| nodes[idx].children.last().copied())
    }

    fn apportion(nodes: &mut [TreeNodePosition], idx: usize, mut default_ancestor: usize) -> usize {
        let Some(prev_sib) = nodes[idx].previous_sibling else {
            return default_ancestor;
        };
        let parent_idx = nodes[idx].parent.expect("Tree invariant broken: no parent");
        let mut inner_right = idx;
        let mut outer_right = idx;
        let mut inner_left = prev_sib;
        let mut outer_left = nodes[parent_idx].children[0];

        let mod_field = nodes[idx].mod_field;
        let mut shift_inner_right = mod_field;
        let mut shift_outer_right = mod_field;
        let mut shift_inner_left = nodes[inner_left].mod_field;
        let mut shift_outer_left = nodes[outer_left].mod_field;
        while let Some(next_inner_left) = Self::next_or_thread(nodes, inner_left)
            && let Some(next_inner_right) = Self::previous_or_thread(nodes, inner_right)
        {
            inner_left = next_inner_left;
            inner_right = next_inner_right;
            outer_left =
                Self::previous_or_thread(nodes, outer_left).expect("Tree invariant broken");
            outer_right = Self::next_or_thread(nodes, outer_right).expect("Tree invariant broken");

            nodes[outer_right].ancestor = idx;

            let shift = (nodes[inner_left].y + shift_inner_left)
                - (nodes[inner_right].y + shift_inner_right)
                + 1.0;
            if shift > 0.0 {
                let ancestor_idx = Self::get_ancestor(nodes, inner_left, idx, default_ancestor);
                Self::move_subtree(nodes, ancestor_idx, idx, shift);
                shift_inner_right += shift;
                shift_outer_right += shift;
            }

            shift_inner_left += nodes[inner_left].mod_field;
            shift_inner_right += nodes[inner_right].mod_field;
            shift_outer_left += nodes[outer_left].mod_field;
        }

        if let Some(next_inner_left) = Self::next_or_thread(nodes, inner_left)
            && Self::next_or_thread(nodes, outer_right).is_none()
        {
            nodes[outer_right].thread = Some(next_inner_left);
            nodes[outer_right].mod_field += shift_inner_left - shift_outer_right;
        } else {
            if let Some(next_inner_right) = Self::previous_or_thread(nodes, inner_right)
                && Self::previous_or_thread(nodes, outer_left).is_none()
            {
                nodes[outer_left].thread = Some(next_inner_right);
                nodes[outer_left].mod_field += shift_inner_right - shift_outer_left;
            }
            default_ancestor = idx;
        }
        default_ancestor
    }

    fn move_subtree(nodes: &mut [TreeNodePosition], left: usize, right: usize, shift: f32) {
        let subtrees = (nodes[right].child_index as f32) - (nodes[left].child_index as f32);
        if subtrees != 0.0 {
            nodes[right].change -= shift / subtrees;
            nodes[left].change += shift / subtrees;
        }
        nodes[right].shift += shift;
        nodes[right].y += shift;
        nodes[right].mod_field += shift;
    }

    fn get_ancestor(
        nodes: &[TreeNodePosition],
        idx: usize,
        other: usize,
        default_ancestor: usize,
    ) -> usize {
        let ancestor = nodes[idx].ancestor;
        let parent_idx = nodes[other].parent.expect("Tree invariant broken");

        if nodes[parent_idx].children.contains(&ancestor) {
            ancestor
        } else {
            default_ancestor
        }
    }

    fn finalize_position(tree: &mut AdvancementRegistry, nodes: &[TreeNodePosition], idx: usize) {
        tree.adv_nodes[nodes[idx].node].set_location(nodes[idx].x as f32, nodes[idx].y);
        for &child_idx in &nodes[idx].children {
            Self::finalize_position(tree, nodes, child_idx);
        }
    }
}
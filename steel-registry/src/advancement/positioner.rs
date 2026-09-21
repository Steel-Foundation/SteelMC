use crate::advancement::registry::AdvancementRegistry;

pub enum PositionError {
    InvalidRootIndex(usize),
    RootMustHaveDisplay(String),
}

impl PositionError {
    #[must_use]
    pub fn get_message(&self) -> String {
        match self {
            PositionError::InvalidRootIndex(index) => format!("Invalid root index at {index}"),
            PositionError::RootMustHaveDisplay(key) => format!("{key} must have display data"),
        }
    }
}

pub type NodePositionIdx = usize;

/// calculate the positions of advancement nodes using the Reingold-Tilford algorithm the same used by minecraft.
///
/// the resulting x position are random so can't really be compared to vanilla
pub fn run(tree: &mut AdvancementRegistry, root_index: usize) -> Result<(), PositionError> {
    let Some(root_node) = tree.adv_nodes.get(root_index) else {
        return Err(PositionError::InvalidRootIndex(root_index));
    };
    if !root_node.has_display() {
        return Err(PositionError::RootMustHaveDisplay(
            root_node.value.key.to_string(),
        ));
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
        r#mod: 0.0,
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
        TreeNodePosition::normalize_y(&mut nodes, -min);
    }

    TreeNodePosition::set_position(tree, &nodes, root_idx);
    Ok(())
}

/// the minecraft code work with reference but due to rust borrow checker it's easier to work with
/// Vector and index but the logic stay the same
struct TreeNodePosition {
    node: usize,
    parent: Option<NodePositionIdx>,
    previous_sibling: Option<NodePositionIdx>,
    child_index: NodePositionIdx,
    children: Vec<NodePositionIdx>,
    ancestor: NodePositionIdx,
    thread: Option<NodePositionIdx>,
    x: i32,
    y: f32,
    r#mod: f32,
    change: f32,
    shift: f32,
}

impl TreeNodePosition {
    /// recursively add a child and skipping the node if it doesn't have a display
    /// # Params
    /// * `nodes` the main vector that register all the [`TreeNodePosition`]
    /// * `tree` the tree that contains every advancement
    /// * `parent_idx` the index of the parent inside `nodes`
    /// * `adv_node_idx` the index of this node inside the `tree`
    /// * `previous_idx` the index inside the `nodes` of the last process brother node.
    ///   `None` if it's the first child to be process
    fn add_child(
        nodes: &mut Vec<TreeNodePosition>,
        tree: &mut AdvancementRegistry,
        parent_idx: NodePositionIdx,
        adv_node_idx: usize,
        mut previous_idx: Option<NodePositionIdx>,
    ) -> Option<NodePositionIdx> {
        let adv_node = tree.adv_nodes.get(adv_node_idx)?;
        if adv_node.has_display() {
            let child_idx = nodes.len();
            let node = &mut nodes[parent_idx];
            let next_child_index = node.children.len() + 1;
            node.children.push(child_idx);

            nodes.push(TreeNodePosition {
                node: adv_node_idx,
                parent: Some(parent_idx),
                previous_sibling: previous_idx,
                child_index: next_child_index,
                children: Vec::new(),
                ancestor: child_idx,
                thread: None,
                x: 0,
                y: -1.0,
                r#mod: 0.0,
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

    /// Traverse every node and compute each y position relative to its siblings and children.
    fn first_walk(nodes: &mut [TreeNodePosition], idx: NodePositionIdx) {
        let num_children = nodes[idx].children.len();
        if num_children == 0 {
            if let Some(prev_sib) = nodes[idx].previous_sibling {
                nodes[idx].y = nodes[prev_sib].y + 1.0;
            } else {
                nodes[idx].y = 0.0;
            }
        } else {
            let mut default_ancestor: Option<NodePositionIdx> = None;
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
                nodes[idx].r#mod = nodes[idx].y - midpoint;
            } else {
                nodes[idx].y = midpoint;
            }
        }
    }

    fn second_walk(
        nodes: &mut [TreeNodePosition],
        idx: NodePositionIdx,
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
        let current_mod = node.r#mod;

        for i in 0..num_children {
            let child_idx = nodes[idx].children[i];
            min = Self::second_walk(nodes, child_idx, mod_sum + current_mod, depth + 1, min);
        }

        min
    }

    fn normalize_y(nodes: &mut [TreeNodePosition], offset: f32) {
        for node in nodes.iter_mut() {
            node.y += offset;
        }
    }

    fn execute_shifts(nodes: &mut [TreeNodePosition], idx: NodePositionIdx) {
        let mut shift = 0.0;
        let mut change = 0.0;

        for &child_idx in nodes[idx].children.iter().rev() {
            nodes[child_idx].y += shift;
            nodes[child_idx].r#mod += shift;
            change += nodes[child_idx].change;
            shift += nodes[child_idx].shift + change;
        }
    }

    #[inline]
    fn previous_or_thread(
        nodes: &[TreeNodePosition],
        idx: NodePositionIdx,
    ) -> Option<NodePositionIdx> {
        nodes[idx]
            .thread
            .or_else(|| nodes[idx].children.first().copied())
    }

    #[inline]
    fn next_or_thread(nodes: &[TreeNodePosition], idx: NodePositionIdx) -> Option<NodePositionIdx> {
        nodes[idx]
            .thread
            .or_else(|| nodes[idx].children.last().copied())
    }

    fn apportion(
        nodes: &mut [TreeNodePosition],
        idx: NodePositionIdx,
        mut default_ancestor: NodePositionIdx,
    ) -> NodePositionIdx {
        let Some(prev_sib) = nodes[idx].previous_sibling else {
            return default_ancestor;
        };
        let parent_idx = nodes[idx].parent.expect("Tree invariant broken: no parent");
        let mut inner_right = idx;
        let mut outer_right = idx;
        let mut inner_left = prev_sib;
        let mut outer_left = nodes[parent_idx].children[0];

        let mod_field = nodes[idx].r#mod;
        let mut shift_inner_right = mod_field;
        let mut shift_outer_right = mod_field;
        let mut shift_inner_left = nodes[inner_left].r#mod;
        let mut shift_outer_left = nodes[outer_left].r#mod;
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

            shift_inner_left += nodes[inner_left].r#mod;
            shift_inner_right += nodes[inner_right].r#mod;
            shift_outer_left += nodes[outer_left].r#mod;
            shift_outer_right += nodes[outer_right].r#mod;
        }

        if let Some(next_inner_left) = Self::next_or_thread(nodes, inner_left)
            && Self::next_or_thread(nodes, outer_right).is_none()
        {
            nodes[outer_right].thread = Some(next_inner_left);
            nodes[outer_right].r#mod += shift_inner_left - shift_outer_right;
        } else {
            // in the real algorithm it doesn't have an else but minecraft had one
            if let Some(next_inner_right) = Self::previous_or_thread(nodes, inner_right)
                && Self::previous_or_thread(nodes, outer_left).is_none()
            {
                nodes[outer_left].thread = Some(next_inner_right);
                nodes[outer_left].r#mod += shift_inner_right - shift_outer_left;
            }
            default_ancestor = idx;
        }
        default_ancestor
    }

    fn move_subtree(
        nodes: &mut [TreeNodePosition],
        left: NodePositionIdx,
        right: NodePositionIdx,
        shift: f32,
    ) {
        let subtrees = (nodes[right].child_index - nodes[left].child_index) as f32;
        if subtrees != 0.0 {
            nodes[right].change -= shift / subtrees;
            nodes[left].change += shift / subtrees;
        }
        nodes[right].shift += shift;
        nodes[right].y += shift;
        nodes[right].r#mod += shift;
    }

    fn get_ancestor(
        nodes: &[TreeNodePosition],
        idx: NodePositionIdx,
        other: NodePositionIdx,
        default_ancestor: NodePositionIdx,
    ) -> NodePositionIdx {
        let ancestor = nodes[idx].ancestor;
        let parent_idx = nodes[other].parent.expect("Tree invariant broken");

        if nodes[parent_idx].children.contains(&ancestor) {
            ancestor
        } else {
            default_ancestor
        }
    }

    fn set_position(
        tree: &mut AdvancementRegistry,
        nodes: &[TreeNodePosition],
        idx: NodePositionIdx,
    ) {
        tree.adv_nodes[nodes[idx].node].set_location(nodes[idx].x as f32, nodes[idx].y);
        for &child_idx in &nodes[idx].children {
            Self::set_position(tree, nodes, child_idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advancement::registry::AdvancementRef;
    use crate::vanilla_advancements::*;
    use steel_utils::Identifier;

    fn get_location(registry: &AdvancementRegistry, key: &Identifier) -> (f32, f32) {
        let loc = registry.get_by_key(key).map(|val| {
            *val.value
                .display
                .as_ref()
                .expect("does not have display")
                .location
                .read()
        });
        loc.unwrap_or_else(|| panic!("unbale to get the location of {key}"))
    }

    #[test]
    fn single_root_no_children() {
        let mut registry = AdvancementRegistry::default();
        registry.register_without_load(&[&STORY_ROOT]);
        let res = run(&mut registry, 0);
        assert!(res.is_ok());
        let location = get_location(&registry, &STORY_ROOT.key);
        assert_eq!(location, (0f32, 0f32));
    }

    #[test]
    fn root_with_linear_children() {
        let mut registry = AdvancementRegistry::default();
        let list: &[AdvancementRef] = &[
            &STORY_ROOT,
            &STORY_MINE_STONE,
            &STORY_UPGRADE_TOOLS,
            &STORY_SMELT_IRON,
        ];
        registry.register_without_load(list);
        let idx = registry.by_key[&list[0].key];
        let res = run(&mut registry, idx);
        assert!(res.is_ok());
        for (i, adv) in list.iter().enumerate() {
            let loc = get_location(&registry, &adv.key);
            assert_eq!(
                loc,
                (i as f32, 0f32),
                "node {} isn't at the right location",
                adv.key
            );
        }
    }

    #[test]
    fn root_with_branching_children() {
        let mut registry = AdvancementRegistry::default();
        let list: &[AdvancementRef] = &[
            &END_ROOT,
            &END_KILL_DRAGON,
            &END_ENTER_END_GATEWAY,
            &END_RESPAWN_DRAGON,
            &END_DRAGON_EGG,
            &END_DRAGON_BREATH,
        ];
        registry.register_without_load(list);
        let idx = registry.by_key[&list[0].key];
        let res = run(&mut registry, idx);
        assert!(res.is_ok());
        let mut locs = [(0f32, 0f32); 6];
        for (i, adv) in list.iter().enumerate() {
            locs[i] = get_location(&registry, &adv.key);
        }
        let expected = [
            (0f32, 1.5f32),
            (1f32, 1.5f32),
            (2f32, 0f32),
            (2f32, 1f32),
            (2f32, 2f32),
            (2f32, 3f32),
        ];
        assert_eq!(locs, expected);
    }
}

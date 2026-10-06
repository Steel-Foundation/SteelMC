//! A declarative builder for assembling [`MenuBehavior`]s.
//!
//! ```rust
//! use steel_registry::{vanilla_items, vanilla_menu_types};
//! use steel_core::{inventory::menu::kinds::BasicKind, player::player_inventory::PlayerInventory};
//!
//! use steel_core::inventory::prelude::*;
//!
//! fn example(container_id: u8, inventory: Shared<PlayerInventory>) -> Menu {
//!     let mut builder = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X1, container_id);
//!
//!     let items = vec![ItemStack::new(&vanilla_items::FLINT_AND_STEEL); 9];
//!     let container = SimpleContainer::from_items(items).into_shared();
//!
//!     let section = builder.section_all(container);
//!
//!     let player = builder.player_inventory(&inventory);
//!     let level_cost = builder.data_slot(0);
//!
//!     builder.route(section, player.all(), FillDirection::Backward);
//!     builder.route(player.all(), section, FillDirection::Forward);
//!
//!     builder.build(BasicKind)
//! }
//! ```

use std::array::IntoIter;
use std::fmt;
use std::iter;
use std::range::Range;
use std::slice;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::vec;

use steel_registry::{item_stack::ItemStack, menu_type::MenuTypeRef};
use steel_utils::locks::Shared;

use crate::inventory::menu::Menu;
use crate::inventory::menu::behavior::MenuBehavior;
use crate::inventory::menu::kind::MenuKind;
use crate::inventory::menu::layout::MenuLayout;
use crate::inventory::{
    lock::{ContainerId, ContainerLockGuard, ContainerRef},
    slots::{NormalSlot, RestrictedRules, RestrictedSlot, ResultHandler, ResultSlot, Slot},
};
use crate::player::Player;
use crate::player::player_inventory::PlayerInventory;

/// Identity of one built menu.
///
/// Given to every [`Section`] and [`DataSlot`] a [`MenuBuilder`] creates, so
/// a handle can never act on a [`Menu`] it wasn't made for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MenuInstanceId(u64);

impl MenuInstanceId {
    /// Creates a new unique `MenuInstanceId`
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// A handle to a contiguous range of slots added to a [`MenuBuilder`].
///
/// Sections contain the id of the [`Menu`] they were made for and can only be
/// created by a builder. Two Sections cannot cover the same range for the same
/// [`Menu`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    menu: MenuInstanceId,
    range: Range<usize>,
}

impl Section {
    pub(crate) fn new(menu: MenuInstanceId, range: impl Into<Range<usize>>) -> Self {
        Self {
            menu,
            range: range.into(),
        }
    }

    /// The start of the section.
    #[must_use]
    pub const fn start(self) -> usize {
        self.range.start
    }

    /// The end of the section.
    #[must_use]
    pub const fn end(self) -> usize {
        self.range.end
    }

    /// The length of the section.
    #[must_use]
    pub const fn len(self) -> usize {
        self.range.end - self.range.start
    }

    /// Whether the section is empty (start == end).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.range.start == self.range.end
    }

    /// Whether the section contains an index.
    #[must_use]
    pub const fn contains(self, slot_index: usize) -> bool {
        slot_index >= self.range.start && slot_index < self.range.end
    }

    /// A copy of the internal range.
    #[must_use]
    pub const fn range(self) -> Range<usize> {
        self.range
    }
}

/// Converts different types into an Iterator of sections so they can be passed into `MenuBuilder::route`
pub trait IntoSections {
    /// The Iterator over the Section(s).
    type Iter: Iterator<Item = Section>;

    /// Converts self into the Iterator.
    fn into_sections(self) -> Self::Iter;
}

impl IntoSections for Section {
    type Iter = iter::Once<Section>;

    fn into_sections(self) -> Self::Iter {
        iter::once(self)
    }
}

impl<const N: usize> IntoSections for [Section; N] {
    type Iter = IntoIter<Section, N>;

    fn into_sections(self) -> Self::Iter {
        self.into_iter()
    }
}

impl<'a> IntoSections for &'a [Section] {
    type Iter = iter::Copied<slice::Iter<'a, Section>>;

    fn into_sections(self) -> Self::Iter {
        self.iter().copied()
    }
}

impl IntoSections for Vec<Section> {
    type Iter = vec::IntoIter<Section>;

    fn into_sections(self) -> Self::Iter {
        self.into_iter()
    }
}

/// The sections that cover the player's inventory.
///
/// Exclusively produced by [`MenuBuilder::player_inventory`].
#[derive(Clone, Copy, Debug)]
pub struct PlayerInventorySections {
    /// All 36 player slots (main and hotbar).
    all: Section,
    /// The 27 main inventory slots.
    main: Section,
    /// The 9 hotbar slots.
    hotbar: Section,
}

impl PlayerInventorySections {
    /// All 36 player slots (main and hotbar).
    #[must_use]
    pub const fn all(&self) -> Section {
        self.all
    }

    /// The 27 main inventory slots.
    #[must_use]
    pub const fn main(&self) -> Section {
        self.main
    }

    /// The 9 hotbar slots.
    #[must_use]
    pub const fn hotbar(&self) -> Section {
        self.hotbar
    }
}

/// A data slot handle created by the [`MenuBuilder::data_slot`], to use for easy access
/// instead of a bare index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataSlot {
    menu: MenuInstanceId,
    index: usize,
}

impl DataSlot {
    /// Reads the current value of this data slot.
    ///
    /// # Panics
    /// Panics if `behavior` belongs to a different menu than the
    /// [`MenuBuilder`] that minted this handle.
    #[must_use]
    pub fn get(self, behavior: &MenuBehavior) -> i16 {
        assert_eq!(
            self.menu,
            behavior.instance(),
            "DataSlot used with a MenuBehavior it does not belong to"
        );
        behavior
            .get_data(self.index)
            .expect("DataSlot index is always valid for its own menu")
    }

    /// Writes a new value to this data slot.
    ///
    /// # Panics
    /// Panics if `behavior` belongs to a different menu than the
    /// [`MenuBuilder`] that minted this handle.
    pub fn set(self, behavior: &mut MenuBehavior, value: i16) {
        assert_eq!(
            self.menu,
            behavior.instance(),
            "DataSlot used with a MenuBehavior it does not belong to"
        );
        behavior.set_data(self.index, value);
    }

    /// The raw data slot index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.index
    }
}

/// The not-yet-carved slots of a container being split across multiple
/// sections.
///
/// Created only by [`MenuBuilder::split`]. Every section created from this handle
/// consumes the next `count` container slots.
pub struct ContainerSlots {
    /// The container being split.
    container: ContainerRef,
    /// The next container slot not yet covered by a section.
    next: usize,
    /// The container's size when [`MenuBuilder::split`] was called, used to
    /// catch sections that take more slots than the container has.
    size: usize,
}

impl fmt::Debug for ContainerSlots {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContainerSlots")
            .field("next", &self.next)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

/// A supplier for ranges of slots in a [`ContainerRef`]. Allowing either making
/// the whole Container a [`Section`] or splitting one Container into multiple Sections.
pub trait SectionSource {
    /// Consumes the next `count` slots of the slot range.
    fn take(self, count: usize) -> (ContainerRef, Range<usize>);
}

impl<T: Into<ContainerRef>> SectionSource for T {
    fn take(self, count: usize) -> (ContainerRef, Range<usize>) {
        (self.into(), (0..count).into())
    }
}

impl SectionSource for &mut ContainerSlots {
    /// # Panics
    /// Panics if taking `count` slots overflows the actual size of the container.
    fn take(self, count: usize) -> (ContainerRef, Range<usize>) {
        let start = self.next;
        assert!(
            start + count <= self.size,
            "section takes container slots {}..{}, but the container only has {} slots",
            start,
            start + count,
            self.size
        );
        self.next = start + count;
        (self.container.clone(), (start..start + count).into())
    }
}

/// Produces the slot for one container index of a section.
pub type SlotFactory = Arc<dyn Fn(&ContainerRef, usize) -> Box<dyn Slot> + Send + Sync>;

/// How a section lowers container indices into menu slots.
///
/// The section methods pick the indices; the kind decides what each index
/// becomes. Accepted by [`MenuBuilder::section_with`], [`MenuBuilder::section_at`],
/// [`MenuBuilder::player_inventory_with`] and grid placements via
/// [`PlacementBuilder::kind`](super::grid::PlacementBuilder::kind).
#[derive(Clone)]
#[non_exhaustive]
pub enum SectionKind {
    /// Plain storage slots.
    Normal,
    /// Placement gated by the rules; pickup gated too when they carry a pickup
    /// predicate. Built by [`restricted`](Self::restricted),
    /// [`guarded`](Self::guarded) and [`take_only`](Self::take_only).
    Restricted(Arc<RestrictedRules>),
    /// No placement, no pickup; clicks are rejected and surface in
    /// `MenuKind::on_slot_clicked`.
    Display,
    /// Slots produced by a caller-supplied factory.
    Custom(SlotFactory),
}

impl SectionKind {
    /// Placement gated by `may_place`, which receives the container-local slot
    /// index; pickup stays allowed.
    pub fn restricted(
        may_place: impl Fn(usize, &ItemStack) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self::Restricted(RestrictedRules::place_only(may_place))
    }

    /// Like [`restricted`](Self::restricted), but pickup is also gated: items
    /// only come out while `may_pickup` returns true.
    pub fn guarded(
        may_place: impl Fn(usize, &ItemStack) -> bool + Send + Sync + 'static,
        may_pickup: impl Fn(usize, &ItemStack, &ContainerLockGuard, &Player) -> bool
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::Restricted(RestrictedRules::guarded(may_place, may_pickup))
    }

    /// Slots produced by `factory` from the section's container and each
    /// covered container index.
    pub fn custom(
        factory: impl Fn(&ContainerRef, usize) -> Box<dyn Slot> + Send + Sync + 'static,
    ) -> Self {
        Self::Custom(Arc::new(factory))
    }

    /// Pickup allowed, placement always rejected — take-only output-style
    /// slots.
    #[must_use]
    pub fn take_only() -> Self {
        Self::Restricted(deny_place_rules())
    }

    pub(crate) fn make(&self, container: &ContainerRef, index: usize) -> Box<dyn Slot> {
        match self {
            Self::Normal => Box::new(NormalSlot::new(container.clone(), index)),
            Self::Restricted(rules) => Box::new(RestrictedSlot::with_rules(
                container.clone(),
                index,
                Arc::clone(rules),
            )),
            Self::Display => Box::new(RestrictedSlot::with_rules(
                container.clone(),
                index,
                deny_all_rules(),
            )),
            Self::Custom(factory) => factory(container, index),
        }
    }
}

impl fmt::Debug for SectionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Normal => "SectionKind::Normal",
            Self::Restricted(_) => "SectionKind::Restricted(..)",
            Self::Display => "SectionKind::Display",
            Self::Custom(_) => "SectionKind::Custom(..)",
        })
    }
}

impl From<&Self> for SectionKind {
    fn from(kind: &Self) -> Self {
        kind.clone()
    }
}

/// Rules that reject placement and allow pickup, shared process-wide.
fn deny_place_rules() -> Arc<RestrictedRules> {
    static DENY: OnceLock<Arc<RestrictedRules>> = OnceLock::new();
    DENY.get_or_init(|| RestrictedRules::place_only(|_, _| false))
        .clone()
}

/// Rules that reject both placement and pickup, shared process-wide.
fn deny_all_rules() -> Arc<RestrictedRules> {
    static DENY: OnceLock<Arc<RestrictedRules>> = OnceLock::new();
    DENY.get_or_init(|| RestrictedRules::guarded(|_, _| false, |_, _, _, _| false))
        .clone()
}

/// The direction in which a slot range is walked when distributing items.
///
/// Vanilla fills backwards when moving into the player inventory so existing
/// hotbar stacks top up first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillDirection {
    /// Walk from the first slot of the range to the last.
    Forward,
    /// Walk from the last slot of the range to the first.
    Backward,
}

/// What to do with fake result output that cannot fit during a shift-click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FakeResultRemainderPolicy {
    /// Drop the unresolved output into the world, as crafting menus do.
    Drop,
    /// Discard the unresolved output after the result handler runs, as anvils do.
    Discard,
}

/// A shift clicking Route that goes from a single Range to a Vec of Ranges.
pub(crate) struct Route {
    pub(crate) from: Range<usize>,
    pub(crate) targets: Vec<Range<usize>>,
    pub(crate) direction: FillDirection,
    pub(crate) fake_result_remainder: FakeResultRemainderPolicy,
}

/// Builds a Menu.
///
/// See the [module documentation](self) for an overview.
pub struct MenuBuilder {
    instance: MenuInstanceId,
    menu_type: Option<MenuTypeRef>,
    container_id: u8,
    overrides_player_slots: bool,
    slots: Vec<Box<dyn Slot>>,
    container_refs: Vec<ContainerRef>,
    data_slots: Vec<i16>,
    routes: Vec<Route>,
    drain_sections: Vec<Range<usize>>,
    /// Container-local slot ranges already covered by a section, used to catch
    /// two sections mapping onto the same container slots.
    claimed: Vec<(ContainerId, Range<usize>)>,
}

impl fmt::Debug for MenuBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MenuBuilder")
            .field("instance", &self.instance)
            .field("container_id", &self.container_id)
            .field("slots", &self.slots.len())
            .field("routes", &self.routes.len())
            .finish_non_exhaustive()
    }
}

impl MenuBuilder {
    /// Creates a new builder for a menu of the given type and container id.
    ///
    /// Pass `None` for the player's own inventory menu, or a menu type
    /// (`&vanilla_menu_types::ANVIL`, ...).
    #[must_use]
    pub fn new(menu_type: impl Into<Option<MenuTypeRef>>, container_id: u8) -> Self {
        Self {
            instance: MenuInstanceId::next(),
            menu_type: menu_type.into(),
            container_id,
            overrides_player_slots: false,
            slots: Vec::new(),
            container_refs: Vec::new(),
            data_slots: Vec::new(),
            routes: Vec::new(),
            drain_sections: Vec::new(),
            claimed: Vec::new(),
        }
    }

    /// Starts splitting a `Container` into multiple sections.
    ///
    /// Use this when you are locked to storing items in one `Container`
    /// and need to split them into different [Section]s.
    ///
    /// # Example
    /// ```rust
    /// use steel_core::inventory::prelude::*;
    /// use steel_core::inventory::menu::kinds::BasicKind;
    ///
    /// let mut b = MenuBuilder::new(None, 0);
    ///
    /// let mut stand = b.split(SimpleContainer::new(5).into_shared());
    /// let bottles = b.section(&mut stand, 3); // slots 0..3
    /// let ingredient = b.section(&mut stand, 1); // slot 3
    /// let fuel = b.section(&mut stand, 1); // slot 4
    ///
    /// b.build(BasicKind);
    /// ```
    ///
    /// # Panics
    /// Panics if the sections carved from the returned handle take more slots
    /// than the container has.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "split is intentionally builder-scoped as part of the menu DSL"
    )]
    pub fn split(&mut self, container: impl Into<ContainerRef>) -> ContainerSlots {
        let container = container.into();
        let size = Self::container_size(&container);
        ContainerSlots {
            container,
            next: 0,
            size,
        }
    }

    /// The container's size, read under a short lock.
    fn container_size(container: &ContainerRef) -> usize {
        ContainerLockGuard::lock_all(slice::from_ref(container))
            .get(container.container_id())
            .expect("container was just locked")
            .get_container_size()
    }

    /// Adds `count` plain slots backed by `source`.
    ///
    /// Pass a container directly to cover its slots `0..count`, or a
    /// [`ContainerSlots`] handle from [`MenuBuilder::split`] to cover the next
    /// `count` slots of a container shared between several sections.
    ///
    /// Returns a [`Section`] handle over the slots that were added.
    ///
    /// # Panics
    /// Panics if the covered container slots overlap another section of this
    /// menu.
    pub fn section(&mut self, source: impl SectionSource, count: usize) -> Section {
        self.section_with(source, count, SectionKind::Normal)
    }

    /// Adds `count` slots backed by `source`, lowered through `kind`.
    ///
    /// # Example
    /// ```rust
    /// use steel_registry::vanilla_items;
    /// use steel_core::inventory::prelude::*;
    /// use steel_core::inventory::menu::kinds::BasicKind;
    ///
    /// let mut b = MenuBuilder::new(None, 0);
    ///
    /// let container = SimpleContainer::new(9).into_shared();
    /// let fuel = b.section_with(container, 9, SectionKind::restricted(|_slot, stack| {
    ///     stack.is(&vanilla_items::COAL)
    /// }));
    ///
    /// b.build(BasicKind);
    /// ```
    ///
    /// # Panics
    /// Panics if the covered container slots overlap another section of this
    /// menu.
    pub fn section_with(
        &mut self,
        source: impl SectionSource,
        count: usize,
        kind: impl Into<SectionKind>,
    ) -> Section {
        let kind = kind.into();
        let (container, range) = source.take(count);
        self.claim(&container, range);
        let start = self.slots.len();
        for index in range {
            let slot = kind.make(&container, index);
            self.push_section_slot(slot, &container, index);
        }
        self.section_from(start)
    }

    /// Adds a section covering every slot of `container`.
    ///
    /// Like [`section`](Self::section) with the container's full size as the
    /// count, so the section can never drift from the container when it is
    /// resized.
    ///
    /// # Panics
    /// Panics if the covered container slots overlap another section of this
    /// menu.
    pub fn section_all(&mut self, container: impl Into<ContainerRef>) -> Section {
        self.section_all_with(container, SectionKind::Normal)
    }

    /// Like [`section_all`](Self::section_all), but lowered through `kind`.
    ///
    /// # Panics
    /// Panics if the covered container slots overlap another section of this
    /// menu.
    pub fn section_all_with(
        &mut self,
        container: impl Into<ContainerRef>,
        kind: impl Into<SectionKind>,
    ) -> Section {
        let container = container.into();
        let size = Self::container_size(&container);
        self.section_with(container, size, kind)
    }

    /// Adds slots over explicit container indices, in the given order.
    ///
    /// The indices may be non-contiguous and in any order; each menu slot maps
    /// to the next index of the iterator. Like [`section_with`](Self::section_with),
    /// the covered indices are claimed against overlapping sections.
    ///
    /// # Panics
    /// Panics if an index repeats or the covered container slots overlap
    /// another section of this menu.
    pub fn section_at(
        &mut self,
        container: impl Into<ContainerRef>,
        indices: impl IntoIterator<Item = usize>,
        kind: impl Into<SectionKind>,
    ) -> Section {
        let kind = kind.into();
        let container = container.into();
        let start = self.slots.len();
        let mut run: Option<Range<usize>> = None;
        for index in indices {
            match &mut run {
                Some(r) if index == r.end => r.end += 1,
                _ => {
                    if let Some(r) = run.take() {
                        self.claim(&container, r);
                    }
                    run = Some((index..index + 1).into());
                }
            }
            let slot = kind.make(&container, index);
            self.push_section_slot(slot, &container, index);
        }
        if let Some(r) = run {
            self.claim(&container, r);
        }
        self.section_from(start)
    }

    /// Adds the player's 36 inventory slots (main inventory then hotbar).
    pub fn player_inventory(
        &mut self,
        inventory: &Shared<PlayerInventory>,
    ) -> PlayerInventorySections {
        self.player_inventory_with(inventory, SectionKind::Normal)
    }

    /// Like [`player_inventory`](Self::player_inventory), but lowers the slots
    /// through `kind`, e.g. [`SectionKind::Display`] for a read-only view of
    /// another player's inventory.
    ///
    /// Player inventory sections never claim their container slots: menus like
    /// invsee legitimately map the same inventory into two sections, and
    /// quick-move skips aliased slots at runtime.
    pub fn player_inventory_with(
        &mut self,
        inventory: &Shared<PlayerInventory>,
        kind: impl Into<SectionKind>,
    ) -> PlayerInventorySections {
        let kind = kind.into();
        let container = ContainerRef::from(inventory.clone());
        let start = self.slots.len();
        for index in PlayerInventory::MAIN.chain(PlayerInventory::HOTBAR) {
            let slot = kind.make(&container, index);
            self.push_section_slot(slot, &container, index);
        }

        let main = Section::new(self.instance, start..start + PlayerInventory::MAIN.len());
        let hotbar = Section::new(
            self.instance,
            start + PlayerInventory::MAIN.len()..self.slots.len(),
        );
        let all = Section::new(self.instance, start..self.slots.len());
        PlayerInventorySections { all, main, hotbar }
    }

    /// Adds a single fake result slot driven by `handler`, backed by the
    /// handler's [`result_container`](ResultHandler::result_container).
    ///
    /// See [`crate::inventory::container::ResultContainer`] and [`crate::inventory::slots::ResultHandler`].
    ///
    /// # Panics
    /// Panics if the result container has no slot `0`, or that slot is already
    /// covered by another section of this menu.
    pub fn result_slot(&mut self, handler: impl ResultHandler + 'static) -> Section {
        let slot = ResultSlot::new(handler);
        let container = slot.result_container().clone();
        self.claim(&container, (0..1).into());
        let start = self.slots.len();
        self.push_section_slot(Box::new(slot), &container, 0);
        self.section_from(start)
    }

    /// Adds raw slots without claiming their container coverage, so tests can
    /// model aliased slots (two menu slots over one container index) the way
    /// [`player_inventory_with`](Self::player_inventory_with) can produce them.
    ///
    /// All slots produced by the iterator have the same concrete type. Use
    /// [`custom_boxed_section`](Self::custom_boxed_section) for a heterogeneous
    /// or already-erased collection.
    ///
    /// Production menus go through [`section_at`](Self::section_at) or a
    /// [`SectionKind::custom`] factory instead, which keep overlap validation.
    #[cfg(test)]
    pub(crate) fn custom_section<S>(&mut self, slots: impl IntoIterator<Item = S>) -> Section
    where
        S: Slot + 'static,
    {
        self.custom_boxed_section(
            slots
                .into_iter()
                .map(|slot| Box::new(slot) as Box<dyn Slot>),
        )
    }

    /// Adds heterogeneous or already-erased slots.
    #[cfg(test)]
    pub(crate) fn custom_boxed_section(
        &mut self,
        slots: impl IntoIterator<Item = Box<dyn Slot>>,
    ) -> Section {
        let start = self.slots.len();
        for slot in slots {
            self.push_boxed_slot(slot);
        }
        self.section_from(start)
    }

    /// Adds a data slot with an initial value and returns a typed handle to it.
    pub fn data_slot(&mut self, initial: i16) -> DataSlot {
        let index = self.data_slots.len();
        self.data_slots.push(initial);
        DataSlot {
            menu: self.instance,
            index,
        }
    }

    /// Declares a shift-click route from each section of `from` into
    /// `targets`.
    ///
    /// Both arguments accept anything [`IntoSections`]: pass a single
    /// [`Section`] directly and use an array/slice/Vec only when there is
    /// genuinely more than one, so brackets signal arity. A multi-section
    /// `from` declares one route per source section.
    ///
    /// Most commonly:
    /// `player_inventory` -> `container` is [`FillDirection::Forward`]
    /// `container` -> `player_inventory` is [`FillDirection::Backward`]
    ///
    /// # Panics
    /// Panics if a section belongs to another builder, a source overlaps an
    /// existing route, or a target overlaps its source.
    pub fn route(
        &mut self,
        from: impl IntoSections,
        targets: impl IntoSections,
        direction: FillDirection,
    ) -> &mut Self {
        self.route_with_remainder_policy(from, targets, direction, FakeResultRemainderPolicy::Drop)
    }

    /// Declares a shift-click route with an explicit fake-result remainder
    /// policy. The policy has no effect on ordinary source slots.
    ///
    /// # Panics
    /// Panics if a section belongs to another builder, a source overlaps an
    /// existing route, or a target overlaps its source.
    pub fn route_with_remainder_policy(
        &mut self,
        from: impl IntoSections,
        targets: impl IntoSections,
        direction: FillDirection,
        fake_result_remainder: FakeResultRemainderPolicy,
    ) -> &mut Self {
        let targets: Vec<Range<usize>> = targets.into_sections().map(|s| self.owned(s)).collect();
        for from in from.into_sections() {
            let from = self.owned(from);
            assert!(
                !self
                    .routes
                    .iter()
                    .any(|route| route.from.start < from.end && from.start < route.from.end),
                "shift-click route source {from:?} overlaps an existing route source",
            );
            assert!(
                !targets
                    .iter()
                    .any(|t| t.start < from.end && from.start < t.end),
                "shift-click route target {targets:?} overlaps its own source {from:?}",
            );
            self.routes.push(Route {
                from,
                targets: targets.clone(),
                direction,
                fake_result_remainder,
            });
        }
        self
    }

    /// Marks `sections` to be emptied back into the player or dropped on the floor on close.
    ///
    /// Accepts anything [`IntoSections`]: pass a single [`Section`] directly
    /// and use an array only for genuinely multiple sections.
    ///
    /// # Panics
    /// Panics if any section was created by a different [`MenuBuilder`].
    ///
    /// # Example
    /// ```rust
    /// use std::sync::Arc;
    ///
    /// use steel_registry::{item_stack::ItemStack, vanilla_items};
    /// use steel_utils::locks::SyncMutex;
    ///
    /// use steel_core::inventory::prelude::*;
    /// use steel_core::inventory::menu::kinds::BasicKind;
    /// use steel_core::inventory::container::SimpleContainer;
    ///
    /// let container_id = 0;
    ///
    /// let mut b = MenuBuilder::new(None, container_id);
    ///
    /// let items = vec![ItemStack::empty(); 9];
    /// let upper_container = SimpleContainer::from_items(items).into_shared();
    ///
    /// let items = vec![ItemStack::new(&vanilla_items::BARRIER); 9];
    /// let lower_container = SimpleContainer::from_items(items).into_shared();
    ///
    /// let display = b.section_with(lower_container, 9, SectionKind::Display);
    ///
    /// let section = b.section(upper_container, 9);
    /// b.drain(section); // only 'section' gets drained when the menu is closed
    /// b.build(BasicKind);
    /// ```
    pub fn drain(&mut self, sections: impl IntoSections) -> &mut Self {
        let ranges: Vec<_> = sections.into_sections().map(|s| self.owned(s)).collect();
        assert!(
            ranges
                .iter()
                .flat_map(|range| *range)
                .all(|slot| !self.slots[slot].is_fake()),
            "drain sections cannot contain fake or result slots"
        );
        self.drain_sections.extend(ranges);
        self
    }

    /// Declares that this menu paints over the client's standard 36 player slots.
    ///
    /// Pending logical inventory updates are deferred while the
    /// menu is open and the slots are restored when it closes.
    pub const fn override_player_slots(&mut self) -> &mut Self {
        self.overrides_player_slots = true;
        self
    }

    /// Consumes the builder, creating the finished [`Menu`].
    ///
    /// # Panics
    /// Panics if the number of slots does not match the client layout declared
    /// by the menu type, or if a fake slot aliases another physical slot.
    #[must_use]
    pub fn build(self, kind: impl MenuKind + 'static) -> Menu {
        self.build_boxed(Box::new(kind))
    }

    /// Consumes the builder using menu behavior selected at runtime.
    ///
    /// This is the erased counterpart to [`Self::build`] for plugin factories
    /// and other callers that already own a boxed menu kind.
    ///
    /// # Panics
    /// Panics if the number of slots does not match the client layout declared
    /// by the menu type, or if a fake slot aliases another physical slot.
    #[must_use]
    pub fn build_boxed(self, kind: Box<dyn MenuKind>) -> Menu {
        if let Some(menu_type) = self.menu_type {
            assert_eq!(
                self.slots.len(),
                menu_type.slot_count,
                "menu type {} expects {} slots, but the builder has {}",
                menu_type.key,
                menu_type.slot_count,
                self.slots.len(),
            );
        }
        Self::assert_no_fake_slot_aliases(&self.slots);

        let mut behavior = MenuBehavior::new(
            self.instance,
            self.slots,
            self.container_id,
            self.menu_type,
            self.container_refs,
        );
        for initial in self.data_slots {
            behavior.add_data_slot(initial);
        }

        let layout = MenuLayout {
            routes: self.routes,
            drain_sections: self.drain_sections,
        };
        Menu::from_parts(behavior, layout, kind, self.overrides_player_slots)
    }

    /// Fake slots have special removal and persistence semantics, so no other
    /// menu slot may expose their physical backing storage.
    fn assert_no_fake_slot_aliases(slots: &[Box<dyn Slot>]) {
        use rustc_hash::FxHashMap;

        let mut physical_slots: FxHashMap<(ContainerId, usize), (usize, bool)> =
            FxHashMap::default();
        for (slot_index, slot) in slots.iter().enumerate() {
            let Some(key) = slot.storage().physical_key() else {
                continue;
            };
            let is_fake = slot.is_fake();
            if let Some(&(other_index, other_is_fake)) = physical_slots.get(&key) {
                assert!(
                    !is_fake && !other_is_fake,
                    "menu slots {other_index} and {slot_index} alias physical container slot \
                     {key:?}, but fake slots require exclusive backing storage"
                );
            } else {
                physical_slots.insert(key, (slot_index, is_fake));
            }
        }
    }

    /// The identity of the menu being built.
    pub(crate) const fn instance(&self) -> MenuInstanceId {
        self.instance
    }

    /// The number of menu slots added so far.
    #[must_use]
    pub const fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Appends a single already-erased slot without creating a section.
    pub(crate) fn push_boxed_slot(&mut self, slot: Box<dyn Slot>) {
        for container in slot.storage().container_refs() {
            self.register_container(container.clone());
        }
        self.slots.push(slot);
    }

    /// Appends a slot whose physical backing must match its declarative source.
    pub(crate) fn push_section_slot(
        &mut self,
        slot: Box<dyn Slot>,
        source: &ContainerRef,
        source_index: usize,
    ) {
        assert_eq!(
            slot.storage().physical_key(),
            Some((source.container_id(), source_index)),
            "section slot backing must match its declared source container and index"
        );
        self.push_boxed_slot(slot);
    }

    /// Records that a section covers the container-local `range` of `container`.
    ///
    /// # Panics
    /// Panics if the range exceeds the container or was already covered by another range.
    pub(crate) fn claim(&mut self, container: &ContainerRef, range: Range<usize>) {
        let id = container.container_id();
        let size = {
            let guard = ContainerLockGuard::lock_all(slice::from_ref(container));
            let Some(container) = guard.get(id) else {
                panic!("container was not locked while validating a menu section");
            };
            container.get_container_size()
        };
        assert!(
            range.end <= size,
            "section takes container slots {}..{}, but the container only has {size} slots",
            range.start,
            range.end,
        );
        for (other_id, other) in &self.claimed {
            assert!(
                *other_id != id || range.start >= other.end || other.start >= range.end,
                "two sections cover overlapping slots ({other:?} and {range:?}) of the same \
                 container; carve shared containers with MenuBuilder::split"
            );
        }
        self.claimed.push((id, range));
    }

    /// Records a container to lock.
    pub(crate) fn register_container(&mut self, container: impl Into<ContainerRef>) {
        let container_ref = container.into();
        let id = container_ref.container_id();
        if !self.container_refs.iter().any(|c| c.container_id() == id) {
            self.container_refs.push(container_ref);
        }
    }

    /// Verifies that `section` was created by this builder.
    fn owned(&self, section: Section) -> Range<usize> {
        assert_eq!(
            section.menu, self.instance,
            "Section was minted by a different MenuBuilder"
        );
        section.range()
    }

    /// Returns a section spanning `start..self.slots.len()`.
    fn section_from(&self, start: usize) -> Section {
        Section::new(self.instance, start..self.slots.len())
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_items, vanilla_menu_types};
    use steel_utils::{Downcast as _, locks::IntoShared};

    use super::*;
    use crate::inventory::container::SimpleContainer;
    use crate::inventory::menu::kinds::BasicKind;

    struct NoopResultHandler(ContainerRef);

    impl ResultHandler for NoopResultHandler {
        fn result_container(&self) -> ContainerRef {
            self.0.clone()
        }

        fn dependencies(&self) -> Vec<ContainerRef> {
            Vec::new()
        }

        fn update_result(&self, _guard: &mut ContainerLockGuard) {}

        fn on_result_taken(
            &self,
            _guard: &mut ContainerLockGuard,
            _player: &Player,
        ) -> Option<ItemStack> {
            None
        }

        fn is_result_valid(&self, _guard: &ContainerLockGuard, _player: &Player) -> bool {
            true
        }
    }

    struct DependencyResultHandler {
        result: ContainerRef,
        dependency: ContainerRef,
    }

    impl ResultHandler for DependencyResultHandler {
        fn result_container(&self) -> ContainerRef {
            self.result.clone()
        }

        fn dependencies(&self) -> Vec<ContainerRef> {
            vec![self.dependency.clone()]
        }

        fn update_result(&self, _guard: &mut ContainerLockGuard) {}

        fn on_result_taken(
            &self,
            _guard: &mut ContainerLockGuard,
            _player: &Player,
        ) -> Option<ItemStack> {
            None
        }

        fn is_result_valid(&self, _guard: &ContainerLockGuard, _player: &Player) -> bool {
            true
        }
    }

    #[test]
    #[should_panic(
        expected = "menu type minecraft:generic_9x6 expects 90 slots, but the builder has 0"
    )]
    fn build_rejects_a_slot_count_that_disagrees_with_the_menu_type() {
        let _ = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X6, 1).build(BasicKind);
    }

    #[test]
    fn builds_with_an_erased_menu_kind() {
        let kind: Box<dyn MenuKind> = Box::new(BasicKind);
        let menu = MenuBuilder::new(None, 0).build_boxed(kind);

        assert!(menu.kind().downcast_ref::<BasicKind>().is_some());
    }

    #[test]
    #[should_panic(
        expected = "section takes container slots 0..2, but the container only has 1 slots"
    )]
    fn direct_section_rejects_a_range_past_container_capacity() {
        let mut builder = MenuBuilder::new(None, 0);
        builder.section(SimpleContainer::new(1).into_shared(), 2);
    }

    #[test]
    #[should_panic(
        expected = "section takes container slots 0..1, but the container only has 0 slots"
    )]
    fn result_slot_rejects_a_container_without_slot_zero() {
        let container = ContainerRef::from(SimpleContainer::new(0).into_shared());
        let mut builder = MenuBuilder::new(None, 0);

        let _ = builder.result_slot(NoopResultHandler(container));
    }

    #[test]
    #[should_panic(expected = "two sections cover overlapping slots")]
    fn result_slot_claims_slot_zero_against_normal_sections() {
        let container = ContainerRef::from(SimpleContainer::new(1).into_shared());
        let mut builder = MenuBuilder::new(None, 0);
        let _ = builder.result_slot(NoopResultHandler(container.clone()));

        let _ = builder.section_all(container);
    }

    #[test]
    fn result_slot_registers_handler_dependencies() {
        let result = ContainerRef::from(SimpleContainer::new(1).into_shared());
        let dependency = ContainerRef::from(SimpleContainer::new(1).into_shared());
        let dependency_id = dependency.container_id();
        let mut builder = MenuBuilder::new(None, 0);
        let _ = builder.result_slot(DependencyResultHandler { result, dependency });

        let menu = builder.build(BasicKind);
        let guard = menu.behavior().lock_all_containers();

        assert!(guard.contains(dependency_id));
    }

    #[test]
    #[should_panic(expected = "drain sections cannot contain fake or result slots")]
    fn drain_rejects_a_result_slot() {
        let result = ContainerRef::from(SimpleContainer::new(1).into_shared());
        let mut builder = MenuBuilder::new(None, 0);
        let section = builder.result_slot(NoopResultHandler(result));

        builder.drain(section);
    }

    #[test]
    #[should_panic(expected = "fake slots require exclusive backing storage")]
    fn build_rejects_a_result_alias_through_player_inventory() {
        let inventory = PlayerInventory::new().into_shared();
        let mut builder = MenuBuilder::new(None, 0);
        let _ = builder.result_slot(NoopResultHandler(ContainerRef::from(inventory.clone())));
        let _ = builder.player_inventory_with(&inventory, SectionKind::Normal);

        let _ = builder.build(BasicKind);
    }

    #[test]
    fn build_allows_non_fake_player_inventory_aliases() {
        let inventory = PlayerInventory::new().into_shared();
        let mut builder = MenuBuilder::new(None, 0);
        let _ = builder.player_inventory(&inventory);
        let _ = builder.player_inventory_with(&inventory, SectionKind::Display);

        let _ = builder.build(BasicKind);
    }

    #[test]
    #[should_panic(expected = "shift-click route source 0..27 overlaps an existing route source")]
    fn route_rejects_overlapping_source_sections() {
        let inventory = PlayerInventory::new().into_shared();
        let mut builder = MenuBuilder::new(None, 0);
        let player = builder.player_inventory(&inventory);
        let target = builder.section(SimpleContainer::new(1).into_shared(), 1);

        builder.route(player.all(), [target], FillDirection::Forward);
        builder.route(player.main(), [target], FillDirection::Forward);
    }

    #[test]
    fn section_at_preserves_the_given_index_order() {
        let container = ContainerRef::from(SimpleContainer::new(5).into_shared());
        let mut b = MenuBuilder::new(None, 0);
        let section = b.section_at(container, [4, 3, 0, 1], SectionKind::Normal);
        let menu = b.build(BasicKind);

        assert_eq!((section.start(), section.end()), (0, 4));
        let container_slots: Vec<usize> = menu
            .behavior()
            .slots()
            .iter()
            .map(|slot| slot.get_container_slot())
            .collect();
        assert_eq!(container_slots, vec![4, 3, 0, 1]);
    }

    #[test]
    #[should_panic(expected = "two sections cover overlapping slots")]
    fn section_at_rejects_indices_claimed_by_another_section() {
        let container = SimpleContainer::new(4).into_shared();
        let mut b = MenuBuilder::new(None, 0);
        let _ = b.section(container.clone(), 2);
        let _ = b.section_at(container, [1], SectionKind::Normal);
    }

    #[test]
    #[should_panic(expected = "two sections cover overlapping slots")]
    fn section_at_rejects_a_repeated_index() {
        let container = ContainerRef::from(SimpleContainer::new(4).into_shared());
        let mut b = MenuBuilder::new(None, 0);
        let _ = b.section_at(container, [0, 2, 0], SectionKind::Normal);
    }

    #[test]
    fn display_kind_rejects_placement() {
        init_vanilla_registry();
        let container = ContainerRef::from(SimpleContainer::new(1).into_shared());
        let mut b = MenuBuilder::new(None, 0);
        let _ = b.section_at(container, [0], SectionKind::Display);
        let menu = b.build(BasicKind);

        let stack = ItemStack::new(&vanilla_items::STONE);
        assert!(!menu.behavior().slots()[0].may_place(&stack));
    }

    #[test]
    fn custom_kind_lowers_through_the_factory() {
        let container = ContainerRef::from(SimpleContainer::new(3).into_shared());
        let factory = SectionKind::custom(|container, index| {
            Box::new(NormalSlot::new(container.clone(), index))
        });
        let mut b = MenuBuilder::new(None, 0);
        let _ = b.section_at(container, [2, 0], factory);
        let menu = b.build(BasicKind);

        let container_slots: Vec<usize> = menu
            .behavior()
            .slots()
            .iter()
            .map(|slot| slot.get_container_slot())
            .collect();
        assert_eq!(container_slots, vec![2, 0]);
    }

    #[test]
    #[should_panic(expected = "section slot backing must match its declared source")]
    fn custom_kind_rejects_a_mismatched_physical_backing() {
        let source = ContainerRef::from(SimpleContainer::new(1).into_shared());
        let other = ContainerRef::from(SimpleContainer::new(1).into_shared());
        let kind =
            SectionKind::custom(move |_container, _index| Box::new(NormalSlot::new(&other, 0)));
        let mut builder = MenuBuilder::new(None, 0);

        let _ = builder.section_with(source, 1, kind);
    }
}

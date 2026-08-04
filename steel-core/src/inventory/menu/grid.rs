//! Builds sections by placing rectangles on a canvas instead of placing sections by hand.
//!
//! Only works with row-major menus, not ones with protocol-defined slot indices like anvils.
//! Those still build through [`MenuBuilder`]'s normal placements.
//!
//! ```rust
//! use steel_registry::{vanilla_items, vanilla_menu_types};
//! use steel_core::inventory::menu::kinds::BasicKind;
//! use steel_core::player::player_inventory::PlayerInventory;
//!
//! use steel_core::inventory::prelude::*;
//!
//! fn example(container_id: u8, inventory: Shared<PlayerInventory>) -> Menu {
//!     let mut b = MenuBuilder::new(&vanilla_menu_types::GENERIC_9X3, container_id);
//!
//!     let storage = SimpleContainer::new(3).into_shared();
//!
//!     let items = b.grid(3, |g| {
//!         let items = g.place(Rect::cols(3..6).rows(1), storage).section();
//!         g.paint_all(&vanilla_items::GRAY_STAINED_GLASS_PANE);
//!         items
//!     });
//!
//!     let player = b.player_inventory(&inventory);
//!     b.route(items, player.all(), FillDirection::Backward);
//!     b.route(player.all(), items, FillDirection::Forward);
//!     b.build(BasicKind)
//! }
//! ```
//!
//! # Rules
//!
//! - Placements never overlap. A cell belongs to at most one placement. A second claim panics.
//! - Paint is decoration. It layers freely (last paint wins) and placements always mask it. Painted cells become locked display slots of one auto-sized filler container.
//! - Every cell must be placed or painted when a scope closes, else panic.
//! - Subgrids are self-contained. [`GridPlacer::subgrid`] has its own local coordinates and coverage check. Parent paint does not reach into it.
//! - [`GridPlacer::carve_rows`], [`GridPlacer::carve_cols`] and [`GridPlacer::rest`] are cursor-computed subgrids. One carve axis per scope. Nest to switch axes.

use std::fmt;
use std::iter::Copied;
use std::ops::{Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive};
use std::slice;

use steel_registry::item_stack::ItemStack;
use steel_utils::locks::IntoShared;

use crate::inventory::container::SimpleContainer;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::builder::{
    IntoSections, MenuBuilder, MenuInstanceId, Section, SectionKind,
};
use crate::inventory::slots::{ResultHandler, ResultSlot, Slot};
use crate::player::Player;

const GRID_WIDTH: usize = 9;

/// A column or row selection for [`Rect::cols`] / [`Rect::rows`]: any range or a bare index.
pub trait SpanBounds: sealed::Sealed {
    /// Lowers to `(start, exclusive end)`. A `None` end means the scope's edge.
    #[doc(hidden)]
    fn bounds(self) -> (usize, Option<usize>);
}

mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for usize {}
impl SpanBounds for usize {
    fn bounds(self) -> (usize, Option<usize>) {
        (self, Some(self + 1))
    }
}

impl sealed::Sealed for Range<usize> {}
impl SpanBounds for Range<usize> {
    fn bounds(self) -> (usize, Option<usize>) {
        (self.start, Some(self.end))
    }
}

impl sealed::Sealed for RangeInclusive<usize> {}
impl SpanBounds for RangeInclusive<usize> {
    fn bounds(self) -> (usize, Option<usize>) {
        let (start, end) = self.into_inner();
        (start, Some(end + 1))
    }
}

impl sealed::Sealed for RangeFrom<usize> {}
impl SpanBounds for RangeFrom<usize> {
    fn bounds(self) -> (usize, Option<usize>) {
        (self.start, None)
    }
}

impl sealed::Sealed for RangeTo<usize> {}
impl SpanBounds for RangeTo<usize> {
    fn bounds(self) -> (usize, Option<usize>) {
        (0, Some(self.end))
    }
}

impl sealed::Sealed for RangeToInclusive<usize> {}
impl SpanBounds for RangeToInclusive<usize> {
    fn bounds(self) -> (usize, Option<usize>) {
        (0, Some(self.end + 1))
    }
}

impl sealed::Sealed for RangeFull {}
impl SpanBounds for RangeFull {
    fn bounds(self) -> (usize, Option<usize>) {
        (0, None)
    }
}

/// Lowers a [`SpanBounds`] to `(start, length)`. A `None` length means the scope's edge.
///
/// # Panics
/// If the range is empty.
fn to_span(axis: &str, span: impl SpanBounds) -> (usize, Option<usize>) {
    let (start, end) = span.bounds();
    let len = end.map(|end| {
        assert!(end > start, "{axis} range {start}..{end} is empty");
        end - start
    });
    (start, len)
}

/// A rectangle of grid cells, selected by column and row ranges. Coordinates
/// are 0-based from the top-left of the scope the rect is used in.
///
/// Built by giving both axes, in either order. See [`SpanBounds`] for the accepted range forms.
///
/// ```rust
/// use steel_core::inventory::menu::Rect;
///
/// Rect::cols(3..6).rows(1);      // columns 3,4,5 of row 1
/// Rect::rows(1..=2).cols(..4);   // the same rect, axes given in the other order
/// Rect::cols(4..).rows(..);      // column 4 to the right edge, all rows
/// Rect::cell(6, 2);              // single cell, shorthand for cols(6).rows(2)
/// ```
///
/// Unbounded ends resolve against the enclosing scope, so the same rect means
/// "to the edge" inside a subgrid too.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    x: usize,
    y: usize,
    /// `None` runs to the scope's right edge.
    w: Option<usize>,
    /// `None` runs to the scope's bottom edge.
    h: Option<usize>,
}

impl Rect {
    /// Starts a rect from a column selection. Finish it with [`ColSpan::rows`].
    ///
    /// # Panics
    /// If the range is empty.
    pub fn cols(cols: impl SpanBounds) -> ColSpan {
        let (x, w) = to_span("column", cols);
        ColSpan { x, w }
    }

    /// Starts a rect from a row selection. Finish it with [`RowSpan::cols`].
    ///
    /// # Panics
    /// If the range is empty.
    pub fn rows(rows: impl SpanBounds) -> RowSpan {
        let (y, h) = to_span("row", rows);
        RowSpan { y, h }
    }

    /// A single cell at column `x`, row `y`.
    #[must_use]
    pub const fn cell(x: usize, y: usize) -> Self {
        Self {
            x,
            y,
            w: Some(1),
            h: Some(1),
        }
    }
}

impl fmt::Debug for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn span(f: &mut fmt::Formatter<'_>, start: usize, len: Option<usize>) -> fmt::Result {
            match len {
                Some(len) => write!(f, "{}..{}", start, start + len),
                None => write!(f, "{start}.."),
            }
        }
        write!(f, "Rect(cols ")?;
        span(f, self.x, self.w)?;
        write!(f, ", rows ")?;
        span(f, self.y, self.h)?;
        write!(f, ")")
    }
}

/// A [`Rect`] with only its columns given. Complete it with [`rows`](Self::rows).
#[derive(Clone, Copy, Debug)]
#[must_use = "give the rect its rows to complete it"]
pub struct ColSpan {
    x: usize,
    w: Option<usize>,
}

impl ColSpan {
    /// Completes the rect with a row selection.
    ///
    /// # Panics
    /// If the range is empty.
    #[must_use]
    pub fn rows(self, rows: impl SpanBounds) -> Rect {
        let (y, h) = to_span("row", rows);
        Rect {
            x: self.x,
            y,
            w: self.w,
            h,
        }
    }
}

/// A [`Rect`] with only its rows given. Complete it with [`cols`](Self::cols).
#[derive(Clone, Copy, Debug)]
#[must_use = "give the rect its columns to complete it"]
pub struct RowSpan {
    y: usize,
    h: Option<usize>,
}

impl RowSpan {
    /// Completes the rect with a column selection.
    ///
    /// # Panics
    /// If the range is empty.
    #[must_use]
    pub fn cols(self, cols: impl SpanBounds) -> Rect {
        let (x, w) = to_span("column", cols);
        Rect {
            x,
            y: self.y,
            w,
            h: self.h,
        }
    }
}

/// A [`Rect`] resolved against a concrete scope: absolute coordinates, concrete extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Abs {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl Abs {
    const fn area(self) -> usize {
        self.w * self.h
    }

    const fn contains_cell(self, x: usize, y: usize) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// Row-major index of `(x, y)` within this rect.
    const fn local_index(self, x: usize, y: usize) -> usize {
        (y - self.y) * self.w + (x - self.x)
    }

    /// Covered cells in row-major order.
    fn cells(self) -> impl Iterator<Item = (usize, usize)> {
        (self.y..self.y + self.h).flat_map(move |y| (self.x..self.x + self.w).map(move |x| (x, y)))
    }
}

/// The sections created by a grid placement.
#[derive(Clone, Debug)]
pub struct Region {
    sections: Vec<Section>,
}

impl Region {
    /// Iterates the sections of this region.
    pub fn iter(&self) -> Copied<slice::Iter<'_, Section>> {
        self.sections.iter().copied()
    }

    /// Whether any section of this region contains the slot index.
    #[must_use]
    pub fn contains(&self, slot_index: usize) -> bool {
        self.sections.iter().any(|s| s.contains(slot_index))
    }

    /// The region's only section.
    ///
    /// # Panics
    /// If the region is not one contiguous slot range.
    #[must_use]
    pub fn single(&self) -> Section {
        assert!(
            self.sections.len() == 1,
            "region covers {} non-contiguous slot ranges; iterate sections() instead",
            self.sections.len()
        );
        self.sections[0]
    }
}

impl<'a> IntoIterator for &'a Region {
    type Item = Section;
    type IntoIter = Copied<slice::Iter<'a, Section>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoSections for &'a Region {
    type Iter = Copied<slice::Iter<'a, Section>>;

    fn into_sections(self) -> Self::Iter {
        self.iter()
    }
}

/// What one grid cell resolved to.
enum Cell {
    /// A leftover `Empty` when a scope closes is a coverage error.
    Empty,
    /// Decoration, backed by the synthesized filler container.
    Painted(ItemStack),
    /// Claimed by the placement at this index into [`GridState::placements`].
    Functional(usize),
}

/// One container-backed placement, in absolute grid coordinates.
struct Placement {
    rect: Abs,
    kind: PlacementKind,
}

enum PlacementKind {
    /// Cell `(x, y)` lowers container slot `mapping.resolve(rect.local_index(x, y))`
    /// through `kind`.
    Section {
        container: ContainerRef,
        mapping: SlotMapping,
        kind: SectionKind,
    },
    /// A single fake result slot driven by a handler.
    Result {
        slot: Option<ResultSlot>,
        container: ContainerRef,
    },
    /// Cell `(x, y)` takes `slots[rect.local_index(x, y)]`, each `Some` until flushed.
    Slots { slots: Vec<Option<Box<dyn Slot>>> },
}

/// Maps a placement's row-major cell index to a container slot index.
enum SlotMapping {
    /// Cell `i` maps to container slot `offset + i`.
    Offset(usize),
    /// Cell `i` maps to container slot `indices[i]`.
    Indices(Vec<usize>),
}

impl SlotMapping {
    fn resolve(&self, local_index: usize) -> usize {
        match self {
            Self::Offset(offset) => offset + local_index,
            Self::Indices(indices) => indices[local_index],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Rows,
    Cols,
}

/// Grid-wide state shared by all nested [`GridPlacer`] scopes.
struct GridState {
    instance: MenuInstanceId,
    /// Flat slot index of the grid's top-left cell in the menu.
    base: usize,
    width: usize,
    cells: Vec<Cell>,
    placements: Vec<Placement>,
}

impl GridState {
    const fn cell_index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }
}

/// One grid scope: the whole grid, or a sub-area inside a subgrid or carve.
struct Frame {
    /// This scope's area, in absolute grid coordinates.
    rect: Abs,
    /// The axis locked in by the first `rows`/`cols` call in this scope.
    axis: Option<Axis>,
    /// Rows or columns already carved off.
    cursor: usize,
    /// Closed subgrids of this scope, in absolute grid coordinates.
    sealed: Vec<Abs>,
}

impl Frame {
    const fn new(rect: Abs) -> Self {
        Self {
            rect,
            axis: None,
            cursor: 0,
            sealed: Vec::new(),
        }
    }
}

/// Places rectangles on a grid scope. Created by [`MenuBuilder::grid`].
///
/// All coordinates are local to this scope, which makes grids combineable.
pub struct GridPlacer<'a> {
    state: &'a mut GridState,
    frame: Frame,
}

impl fmt::Debug for GridPlacer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GridPlacer")
            .field("width", &self.width())
            .field("height", &self.height())
            .finish_non_exhaustive()
    }
}

/// A pending placement, applied only when [`region`](Self::region) or
/// [`result`](Self::result) is called.
#[must_use = "a placement does nothing until .region() or .result() is called"]
pub struct PlacementBuilder<'p, 'a> {
    grid: &'p mut GridPlacer<'a>,
    rect: Rect,
    container: ContainerRef,
    mapping: SlotMapping,
    kind: SectionKind,
}

impl PlacementBuilder<'_, '_> {
    /// Maps the rect's first cell to container slot `slot` instead of 0.
    pub fn start_at(mut self, slot: usize) -> Self {
        self.mapping = SlotMapping::Offset(slot);
        self
    }

    /// Maps the rect's cells (row-major) to these container slots, in the
    /// given order. Replaces [`start_at`](Self::start_at).
    ///
    /// The committing call panics if the count differs from the rect's cell
    /// count.
    pub fn at_indices(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        self.mapping = SlotMapping::Indices(indices.into_iter().collect());
        self
    }

    /// Lowers the cells through `kind`, e.g. a [`SectionKind::Custom`] factory.
    /// The generalization of [`restrict`](Self::restrict), [`guard`](Self::guard)
    /// and [`display`](Self::display).
    pub fn kind(mut self, kind: impl Into<SectionKind>) -> Self {
        self.kind = kind.into();
        self
    }

    /// Only accepts items passing `may_place`. Pickup stays allowed.
    pub fn restrict(
        self,
        may_place: impl Fn(usize, &ItemStack) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.kind(SectionKind::restricted(may_place))
    }

    /// Guards both placement and pickup.
    pub fn guard(
        self,
        may_place: impl Fn(usize, &ItemStack) -> bool + Send + Sync + 'static,
        may_pickup: impl Fn(usize, &ItemStack, &ContainerLockGuard, &Player) -> bool
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.kind(SectionKind::guarded(may_place, may_pickup))
    }

    /// Locks the cells as display slots. Clicks are rejected and handled in `MenuKind::on_slot_clicked`.
    pub fn display(self) -> Self {
        self.kind(SectionKind::Display)
    }

    /// Commits the placement and returns its region.
    #[must_use = "hold the region to route or gate its slots"]
    pub fn region(self) -> Region {
        let Self {
            grid,
            rect,
            container,
            mapping,
            kind,
        } = self;
        grid.place_section(rect, container, mapping, kind)
    }

    /// Commits the placement and returns its single contiguous [`Section`].
    ///
    /// Shorthand for [`region()`](Self::region) followed by
    /// [`Region::single`]; use `region()` when the rect may lower to multiple
    /// slot ranges.
    ///
    /// # Panics
    /// Panics if the placement lowers to more than one contiguous slot range.
    /// Single-cell and single-row rects can never trip this.
    #[must_use = "hold the section to route or gate its slots"]
    pub fn section(self) -> Section {
        self.region().single()
    }

    /// Commits a single fake result slot driven by `handler`, ignoring `start_at` and guards.
    ///
    /// # Panics
    /// If the rect is not a single cell, or the placement container differs
    /// from [`ResultHandler::result_container`].
    pub fn result(self, handler: impl ResultHandler + 'static) -> Section {
        let slot = ResultSlot::new(handler);
        let result_container = slot.result_container().clone();
        assert_eq!(
            self.container.container_id(),
            result_container.container_id(),
            "result placement container must match ResultHandler::result_container"
        );
        self.grid.place_result(self.rect, slot, result_container)
    }
}

impl<'a> GridPlacer<'a> {
    /// The number of columns in this scope.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.frame.rect.w
    }

    /// The number of rows in this scope.
    #[must_use]
    pub const fn height(&self) -> usize {
        self.frame.rect.h
    }

    /// The rect covering this whole scope.
    #[must_use]
    pub const fn full(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: Some(self.frame.rect.w),
            h: Some(self.frame.rect.h),
        }
    }

    /// The resolved size of `rect` as `(columns, rows)`, unbounded ends run to the edges.
    ///
    /// # Panics
    /// If the rect does not fit this scope.
    #[must_use]
    pub fn size_of(&self, rect: Rect) -> (usize, usize) {
        let abs = self.to_abs(rect);
        (abs.w, abs.h)
    }

    /// Starts placing slots for `container` over `rect`, first cell at container slot 0.
    ///
    /// Configure the covered container slots with [`start_at`](PlacementBuilder::start_at) or
    /// [`at_indices`](PlacementBuilder::at_indices), the slot behavior with
    /// [`restrict`](PlacementBuilder::restrict), [`guard`](PlacementBuilder::guard),
    /// [`display`](PlacementBuilder::display) or [`kind`](PlacementBuilder::kind), then commit with
    /// [`section`](PlacementBuilder::section), [`region`](PlacementBuilder::region) or
    /// [`result`](PlacementBuilder::result).
    pub fn place(
        &mut self,
        rect: Rect,
        container: impl Into<ContainerRef>,
    ) -> PlacementBuilder<'_, 'a> {
        PlacementBuilder {
            grid: self,
            rect,
            container: container.into(),
            mapping: SlotMapping::Offset(0),
            kind: SectionKind::Normal,
        }
    }

    /// # Panics
    /// If the rect exceeds this scope, overlaps another placement or subgrid,
    /// the container is too small, or an explicit index mapping does not match
    /// the rect's cell count.
    fn place_section(
        &mut self,
        rect: Rect,
        container: ContainerRef,
        mapping: SlotMapping,
        kind: SectionKind,
    ) -> Region {
        Self::assert_mapping(&container, self.to_abs(rect), &mapping);
        self.claim_functional(
            rect,
            PlacementKind::Section {
                container,
                mapping,
                kind,
            },
        )
    }

    /// Adds concrete pre-built slots over `rect`, consumed in row-major order.
    ///
    /// The menu derives its lock set from each slot's
    /// [`SlotStorage`](crate::inventory::slots::SlotStorage).
    /// Use [`place_boxed_slots`](Self::place_boxed_slots) for a heterogeneous or
    /// already-erased collection.
    ///
    /// # Panics
    /// If the slot count differs from the rect's cell count, or on the overlap/bounds conditions of [`place`](Self::place).
    pub fn place_slots<S>(&mut self, rect: Rect, slots: impl IntoIterator<Item = S>) -> Region
    where
        S: Slot + 'static,
    {
        self.place_boxed_slots(
            rect,
            slots
                .into_iter()
                .map(|slot| Box::new(slot) as Box<dyn Slot>),
        )
    }

    /// Adds heterogeneous or already-erased slots over `rect` in row-major order.
    ///
    /// # Panics
    /// If the slot count differs from the rect's cell count, or on the overlap/bounds conditions of [`place`](Self::place).
    pub fn place_boxed_slots(
        &mut self,
        rect: Rect,
        slots: impl IntoIterator<Item = Box<dyn Slot>>,
    ) -> Region {
        let slots: Vec<Option<Box<dyn Slot>>> = slots.into_iter().map(Some).collect();
        let abs = self.to_abs(rect);
        assert!(
            slots.len() == abs.area(),
            "place_slots got {} slots for a {}x{} rect ({} cells)",
            slots.len(),
            abs.w,
            abs.h,
            abs.area()
        );
        self.claim_functional(rect, PlacementKind::Slots { slots })
    }

    /// # Panics
    /// If the rect is not a single cell, or on the overlap/bounds conditions of a placement.
    fn place_result(&mut self, at: Rect, slot: ResultSlot, container: ContainerRef) -> Section {
        let abs = self.to_abs(at);
        assert!(
            abs.area() == 1,
            "result placement requires a single cell, got a {}x{} rect",
            abs.w,
            abs.h
        );
        let region = self.claim_functional(
            at,
            PlacementKind::Result {
                slot: Some(slot),
                container,
            },
        );
        region.single()
    }

    /// Paints decoration over `rect`. Painted cells become locked display slots of one filler container.
    ///
    /// Paint is the bottom layer. Placements and subgrids mask it regardless of call order, and the last paint on a cell wins.
    ///
    /// # Panics
    /// If the rect exceeds this scope.
    pub fn paint(&mut self, rect: Rect, stack: impl Into<ItemStack>) {
        let stack = stack.into();
        let abs = self.to_abs(rect);
        for (x, y) in abs.cells() {
            if self.in_sealed(x, y) {
                continue;
            }
            let index = self.state.cell_index(x, y);
            if !matches!(self.state.cells[index], Cell::Functional(_)) {
                self.state.cells[index] = Cell::Painted(stack.clone());
            }
        }
    }

    /// Paints the whole scope.
    pub fn paint_all(&mut self, stack: impl Into<ItemStack>) {
        self.paint(self.full(), stack);
    }

    /// Runs `f` against the sub-area `rect` with its own local coordinates.
    ///
    /// Self-contained. It must fully cover its own area, parent paint does not reach in, and nothing may be placed over it afterwards.
    ///
    /// # Panics
    /// If the rect exceeds this scope or overlaps a placement or subgrid, or if `f` leaves cells uncovered.
    pub fn subgrid<R>(&mut self, rect: Rect, f: impl FnOnce(&mut GridPlacer<'_>) -> R) -> R {
        let abs = self.to_abs(rect);
        for (x, y) in abs.cells() {
            assert!(
                !self.in_sealed(x, y),
                "subgrid {rect:?} overlaps another subgrid at local cell ({}, {})",
                x - self.frame.rect.x,
                y - self.frame.rect.y
            );
            let index = self.state.cell_index(x, y);
            match self.state.cells[index] {
                Cell::Functional(_) => panic!(
                    "subgrid {rect:?} overlaps a placement at local cell ({}, {})",
                    x - self.frame.rect.x,
                    y - self.frame.rect.y
                ),
                // The subgrid owns its area and must cover it itself.
                Cell::Painted(_) => self.state.cells[index] = Cell::Empty,
                Cell::Empty => {}
            }
        }

        let mut child = GridPlacer {
            state: &mut *self.state,
            frame: Frame::new(abs),
        };
        let result = f(&mut child);
        child.check_coverage();
        self.frame.sealed.push(abs);
        result
    }

    /// Carves the next `count` rows off this scope and runs `f` against them.
    ///
    /// # Panics
    /// If `carve_cols` was already used here, if fewer than `count` rows remain, or on the [`subgrid`](Self::subgrid) conditions.
    pub fn carve_rows<R>(&mut self, count: usize, f: impl FnOnce(&mut GridPlacer<'_>) -> R) -> R {
        self.carve(Axis::Rows, count, f)
    }

    /// Carves the next `count` columns off this scope and runs `f` against them.
    ///
    /// # Panics
    /// If `carve_rows` was already used here, if fewer than `count` columns remain, or on the [`subgrid`](Self::subgrid) conditions.
    pub fn carve_cols<R>(&mut self, count: usize, f: impl FnOnce(&mut GridPlacer<'_>) -> R) -> R {
        self.carve(Axis::Cols, count, f)
    }

    /// Carves everything remaining on the current axis and runs `f` against it.
    ///
    /// # Panics
    /// If nothing remains to carve, or on the [`subgrid`](Self::subgrid) conditions.
    pub fn rest<R>(&mut self, f: impl FnOnce(&mut GridPlacer<'_>) -> R) -> R {
        let axis = self.frame.axis.unwrap_or(Axis::Rows);
        let remaining = match axis {
            Axis::Rows => self.height() - self.frame.cursor,
            Axis::Cols => self.width() - self.frame.cursor,
        };
        assert!(
            remaining > 0,
            "rest() called with nothing remaining to carve"
        );
        self.carve(axis, remaining, f)
    }

    fn carve<R>(
        &mut self,
        axis: Axis,
        count: usize,
        f: impl FnOnce(&mut GridPlacer<'_>) -> R,
    ) -> R {
        assert!(count > 0, "cannot carve zero rows/columns");
        assert!(
            self.frame.axis.is_none_or(|a| a == axis),
            "cannot mix rows() and cols() in one grid scope; open a subgrid to switch axes"
        );
        let (remaining, local) = match axis {
            Axis::Rows => (
                self.height() - self.frame.cursor,
                Rect {
                    x: 0,
                    y: self.frame.cursor,
                    w: None,
                    h: Some(count),
                },
            ),
            Axis::Cols => (
                self.width() - self.frame.cursor,
                Rect {
                    x: self.frame.cursor,
                    y: 0,
                    w: Some(count),
                    h: None,
                },
            ),
        };
        assert!(
            count <= remaining,
            "carving {count} {} exceeds the {remaining} remaining",
            match axis {
                Axis::Rows => "rows",
                Axis::Cols => "columns",
            }
        );
        self.frame.axis = Some(axis);
        self.frame.cursor += count;
        self.subgrid(local, f)
    }

    /// Resolves a scope-local rect to absolute coordinates, unbounded ends running to the scope's edges.
    ///
    /// # Panics
    /// If the rect does not fit this scope.
    fn to_abs(&self, rect: Rect) -> Abs {
        let frame = self.frame.rect;
        let w = rect.w.unwrap_or_else(|| frame.w.saturating_sub(rect.x));
        let h = rect.h.unwrap_or_else(|| frame.h.saturating_sub(rect.y));
        assert!(
            w > 0 && h > 0 && rect.x + w <= frame.w && rect.y + h <= frame.h,
            "rect {rect:?} exceeds the {}x{} grid area",
            frame.w,
            frame.h
        );
        Abs {
            x: frame.x + rect.x,
            y: frame.y + rect.y,
            w,
            h,
        }
    }

    /// Whether the cell `(x, y)` lies in a closed subgrid of this scope.
    fn in_sealed(&self, x: usize, y: usize) -> bool {
        self.frame.sealed.iter().any(|r| r.contains_cell(x, y))
    }

    /// Claims `rect` for a placement and mints its region.
    fn claim_functional(&mut self, rect: Rect, kind: PlacementKind) -> Region {
        let abs = self.to_abs(rect);
        for (x, y) in abs.cells() {
            assert!(
                !self.in_sealed(x, y),
                "rect {rect:?} overlaps a subgrid at local cell ({}, {})",
                x - self.frame.rect.x,
                y - self.frame.rect.y
            );
            assert!(
                !matches!(
                    self.state.cells[self.state.cell_index(x, y)],
                    Cell::Functional(_)
                ),
                "rect {rect:?} overlaps another placement at local cell ({}, {})",
                x - self.frame.rect.x,
                y - self.frame.rect.y
            );
        }

        let placement = self.state.placements.len();
        for (x, y) in abs.cells() {
            let index = self.state.cell_index(x, y);
            self.state.cells[index] = Cell::Functional(placement);
        }
        self.state.placements.push(Placement { rect: abs, kind });
        self.region_for(abs)
    }

    /// Mints the sections covering `abs`, one per row, with flat-adjacent rows merged.
    fn region_for(&self, abs: Abs) -> Region {
        let mut sections: Vec<(usize, usize)> = Vec::new();
        for y in abs.y..abs.y + abs.h {
            let start = self.state.base + y * self.state.width + abs.x;
            match sections.last_mut() {
                Some(last) if last.1 == start => last.1 = start + abs.w,
                _ => sections.push((start, start + abs.w)),
            }
        }
        Region {
            sections: sections
                .into_iter()
                .map(|(start, end)| Section::new(self.state.instance, start..end))
                .collect(),
        }
    }

    /// Panics if any cell of this scope is still [`Cell::Empty`].
    fn check_coverage(&self) {
        let holes: Vec<(usize, usize)> = self
            .frame
            .rect
            .cells()
            .filter(|&(x, y)| matches!(self.state.cells[self.state.cell_index(x, y)], Cell::Empty))
            .map(|(x, y)| (x - self.frame.rect.x, y - self.frame.rect.y))
            .collect();
        assert!(
            holes.is_empty(),
            "grid area not fully covered; place or paint the local cells (column, row): {holes:?}"
        );
    }

    /// Asserts that the mapping fits the placement's rect and the container.
    fn assert_mapping(container: &ContainerRef, rect: Abs, mapping: &SlotMapping) {
        use crate::inventory::lock::ContainerLockGuard;

        let size = ContainerLockGuard::lock_all(slice::from_ref(container))
            .get(container.container_id())
            .expect("container was just locked")
            .get_container_size();
        match mapping {
            SlotMapping::Offset(offset) => assert!(
                offset + rect.area() <= size,
                "placement needs container slots {}..{}, but the container only has {size} slots",
                offset,
                offset + rect.area()
            ),
            SlotMapping::Indices(indices) => {
                assert!(
                    indices.len() == rect.area(),
                    "at_indices got {} slots for a {}x{} rect ({} cells)",
                    indices.len(),
                    rect.w,
                    rect.h,
                    rect.area()
                );
                for &index in indices {
                    assert!(
                        index < size,
                        "at_indices maps to container slot {index}, but the container only has {size} slots"
                    );
                }
            }
        }
    }
}

impl MenuBuilder {
    /// Runs `f` against a fresh 9-wide, `rows`-tall grid and appends its slots in row-major order.
    ///
    /// Grids compose. Each call covers the next `rows` rows of the menu. See the
    /// [module documentation](self) for placement rules and an example.
    ///
    /// # Panics
    /// If `rows` is zero, if the slots so far do not fill complete rows, or if `f` leaves cells neither placed nor painted.
    pub fn grid<R>(&mut self, rows: usize, f: impl FnOnce(&mut GridPlacer<'_>) -> R) -> R {
        assert!(rows > 0, "grid needs at least one row");
        assert!(
            self.slot_count().is_multiple_of(GRID_WIDTH),
            "grid starts mid-row (slot {}); previous sections must fill complete rows of {GRID_WIDTH}",
            self.slot_count()
        );

        let mut state = GridState {
            instance: self.instance(),
            base: self.slot_count(),
            width: GRID_WIDTH,
            cells: (0..GRID_WIDTH * rows).map(|_| Cell::Empty).collect(),
            placements: Vec::new(),
        };
        let mut placer = GridPlacer {
            state: &mut state,
            frame: Frame::new(Abs {
                x: 0,
                y: 0,
                w: GRID_WIDTH,
                h: rows,
            }),
        };
        let result = f(&mut placer);
        placer.check_coverage();
        self.flush_grid(state);
        result
    }

    /// Emits the resolved grid cells as menu slots in row-major order.
    fn flush_grid(&mut self, state: GridState) {
        let GridState {
            cells,
            mut placements,
            width,
            ..
        } = state;
        for placement in &placements {
            match &placement.kind {
                PlacementKind::Section {
                    container, mapping, ..
                } => match mapping {
                    SlotMapping::Offset(offset) => {
                        self.claim(container, (*offset..offset + placement.rect.area()).into());
                    }
                    SlotMapping::Indices(indices) => {
                        for &index in indices {
                            self.claim(container, (index..index + 1).into());
                        }
                    }
                },
                PlacementKind::Result { container, .. } => {
                    self.claim(container, (0..1).into());
                }
                PlacementKind::Slots { .. } => {}
            }
        }

        let painted: Vec<ItemStack> = cells
            .iter()
            .filter_map(|cell| match cell {
                Cell::Painted(stack) => Some(stack.clone()),
                _ => None,
            })
            .collect();
        let filler = (!painted.is_empty())
            .then(|| ContainerRef::from(SimpleContainer::from_items(painted).into_shared()));

        let mut filler_next = 0;
        for (index, cell) in cells.iter().enumerate() {
            let (x, y) = (index % width, index / width);
            match cell {
                Cell::Empty => unreachable!("coverage was checked before flushing"),
                Cell::Painted(_) => {
                    let container = filler
                        .as_ref()
                        .expect("filler exists when cells are painted");
                    let slot = SectionKind::Display.make(container, filler_next);
                    self.push_section_slot(slot, container, filler_next);
                    filler_next += 1;
                }
                Cell::Functional(placement) => {
                    let Placement { rect, kind } = &mut placements[*placement];
                    match kind {
                        PlacementKind::Section {
                            container,
                            mapping,
                            kind,
                        } => {
                            let container_index = mapping.resolve(rect.local_index(x, y));
                            let slot = kind.make(container, container_index);
                            self.push_section_slot(slot, container, container_index);
                        }
                        PlacementKind::Result { slot, container } => {
                            let slot = slot
                                .take()
                                .expect("each result placement maps to exactly one slot");
                            self.push_section_slot(Box::new(slot), container, 0);
                        }
                        PlacementKind::Slots { slots, .. } => {
                            let slot = slots[rect.local_index(x, y)]
                                .take()
                                .expect("each grid cell maps to exactly one slot");
                            self.push_boxed_slot(slot);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{
        lock::ContainerLockGuard,
        slots::{NormalSlot, ResultHandler},
    };
    use crate::player::Player;
    use steel_utils::locks::IntoShared;

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

    fn container(size: usize) -> ContainerRef {
        ContainerRef::from(SimpleContainer::new(size).into_shared())
    }

    fn ranges(region: &Region) -> Vec<(usize, usize)> {
        region.iter().map(|s| (s.start(), s.end())).collect()
    }

    #[test]
    fn full_width_placement_merges_into_one_section() {
        let mut b = MenuBuilder::new(None, 0);
        let region = b.grid(2, |g| g.place(g.full(), container(18)).region());
        assert_eq!(ranges(&region), vec![(0, 18)]);
        assert_eq!(b.slot_count(), 18);
    }

    #[test]
    fn narrow_placement_yields_one_section_per_row() {
        let mut b = MenuBuilder::new(None, 0);
        let region = b.grid(3, |g| {
            let region = g.place(Rect::cols(1..4).rows(..), container(9)).region();
            g.paint_all(ItemStack::empty());
            region
        });
        assert_eq!(ranges(&region), vec![(1, 4), (10, 13), (19, 22)]);
        assert_eq!(b.slot_count(), 27);
    }

    #[test]
    fn place_slots_lands_in_row_major_order() {
        use crate::inventory::menu::kinds::BasicKind;

        let c = container(9);
        let slots: Vec<NormalSlot> = (3..7).map(|i| NormalSlot::new(c.clone(), i)).collect();

        let mut b = MenuBuilder::new(None, 0);
        let region = b.grid(1, |g| {
            let region = g.place_slots(Rect::cols(0..4).rows(..), slots);
            g.paint_all(ItemStack::empty());
            region
        });

        assert_eq!(ranges(&region), vec![(0, 4)]);
        assert_eq!(b.slot_count(), 9);

        let menu = b.build(BasicKind);
        let keys: Vec<usize> = (0..4)
            .map(|menu_slot| {
                menu.behavior().slots()[menu_slot]
                    .storage()
                    .physical_key()
                    .expect("place_slots slots are container-backed")
                    .1
            })
            .collect();
        assert_eq!(keys, vec![3, 4, 5, 6]);
    }

    #[test]
    fn at_indices_maps_cells_in_the_given_order() {
        use crate::inventory::menu::kinds::BasicKind;

        let mut b = MenuBuilder::new(None, 0);
        let region = b.grid(1, |g| {
            let region = g
                .place(Rect::cols(0..4).rows(..), container(9))
                .at_indices([8, 6, 4, 2])
                .region();
            g.paint_all(ItemStack::empty());
            region
        });

        assert_eq!(ranges(&region), vec![(0, 4)]);

        let menu = b.build(BasicKind);
        let keys: Vec<usize> = (0..4)
            .map(|menu_slot| {
                menu.behavior().slots()[menu_slot]
                    .storage()
                    .physical_key()
                    .expect("at_indices slots are container-backed")
                    .1
            })
            .collect();
        assert_eq!(keys, vec![8, 6, 4, 2]);
    }

    #[test]
    #[should_panic(expected = "at_indices got 3 slots for a 4x1 rect (4 cells)")]
    fn at_indices_rejects_a_count_that_disagrees_with_the_rect() {
        let mut b = MenuBuilder::new(None, 0);
        b.grid(1, |g| {
            let _ = g
                .place(Rect::cols(0..4).rows(..), container(9))
                .at_indices([0, 1, 2])
                .region();
        });
    }

    #[test]
    fn kind_lowers_cells_through_a_custom_factory() {
        use crate::inventory::menu::kinds::BasicKind;

        let factory = SectionKind::custom(|container, index| {
            Box::new(NormalSlot::new(container.clone(), index))
        });

        let mut b = MenuBuilder::new(None, 0);
        b.grid(1, |g| {
            let _ = g
                .place(Rect::cols(0..2).rows(..), container(2))
                .kind(factory)
                .region();
            g.paint_all(ItemStack::empty());
        });

        let menu = b.build(BasicKind);
        let keys: Vec<usize> = (0..2)
            .map(|menu_slot| {
                menu.behavior().slots()[menu_slot]
                    .storage()
                    .physical_key()
                    .expect("custom factory slots are container-backed")
                    .1
            })
            .collect();
        assert_eq!(keys, vec![0, 1]);
    }

    #[test]
    #[should_panic(expected = "place_slots got 3 slots")]
    fn place_slots_panics_on_count_mismatch() {
        let c = container(9);
        let slots: Vec<NormalSlot> = (0..3).map(|i| NormalSlot::new(c.clone(), i)).collect();

        let mut b = MenuBuilder::new(None, 0);
        b.grid(1, |g| {
            g.place_slots(Rect::cols(0..4).rows(..), slots);
            g.paint_all(ItemStack::empty());
        });
    }

    #[test]
    fn sibling_grids_stack_vertically() {
        let mut b = MenuBuilder::new(None, 0);
        let top = b.grid(1, |g| g.place(g.full(), container(9)).region());
        let bottom = b.grid(1, |g| g.place(g.full(), container(9)).region());
        assert_eq!(ranges(&top), vec![(0, 9)]);
        assert_eq!(ranges(&bottom), vec![(9, 18)]);
    }

    #[test]
    fn cols_carve_side_by_side() {
        let mut b = MenuBuilder::new(None, 0);
        let (left, mid, right) = b.grid(2, |g| {
            let left = g.carve_cols(4, |g| g.place(g.full(), container(8)).region());
            let mid = g.carve_cols(1, |g| g.place(g.full(), container(2)).region());
            let right = g.rest(|g| g.place(g.full(), container(8)).region());
            (left, mid, right)
        });
        assert_eq!(ranges(&left), vec![(0, 4), (9, 13)]);
        assert_eq!(ranges(&mid), vec![(4, 5), (13, 14)]);
        assert_eq!(ranges(&right), vec![(5, 9), (14, 18)]);
    }

    #[test]
    fn rows_and_offset_carve_one_container() {
        let mut b = MenuBuilder::new(None, 0);
        let shared = container(54);
        let (top, body) = b.grid(6, |g| {
            let top = g.carve_rows(1, |g| g.place(g.full(), shared.clone()).region());
            let body = g.rest(|g| g.place(g.full(), shared.clone()).start_at(9).region());
            (top.single(), body.single())
        });
        assert_eq!((top.start(), top.end()), (0, 9));
        assert_eq!((body.start(), body.end()), (9, 54));
    }

    #[test]
    fn restricted_placement_covers_like_place() {
        let mut b = MenuBuilder::new(None, 0);
        let region = b.grid(2, |g| {
            let region = g
                .place(Rect::cols(2..5).rows(..), container(6))
                .guard(|_slot, _stack| true, |_, _, _, _| false)
                .region();
            g.paint_all(ItemStack::empty());
            region
        });
        assert_eq!(ranges(&region), vec![(2, 5), (11, 14)]);
        assert_eq!(b.slot_count(), 18);
    }

    #[test]
    fn placements_mask_paint_in_any_order() {
        let mut b = MenuBuilder::new(None, 0);
        b.grid(2, |g| {
            g.paint_all(ItemStack::empty());
            let _ = g.place(Rect::cols(0..2).rows(0), container(2)).region();
            let _ = g.place(Rect::cols(2..4).rows(0), container(2)).region();
        });
        assert_eq!(b.slot_count(), 18);
    }

    #[test]
    fn result_slot_lands_on_its_cell() {
        use crate::inventory::container::ResultContainer;

        let container = ContainerRef::from(ResultContainer::new().into_shared());
        let mut b = MenuBuilder::new(None, 0);
        let result = b.grid(3, |g| {
            let result = g
                .place(Rect::cell(6, 2), container.clone())
                .result(NoopResultHandler(container.clone()));
            g.paint_all(ItemStack::empty());
            result
        });
        assert_eq!((result.start(), result.end()), (24, 25));
    }

    #[test]
    #[should_panic(
        expected = "result placement container must match ResultHandler::result_container"
    )]
    fn result_placement_rejects_a_container_that_differs_from_the_handler() {
        let placed = container(1);
        let handled = container(1);
        let mut b = MenuBuilder::new(None, 0);

        b.grid(1, |g| {
            let _ = g
                .place(Rect::cell(0, 0), placed)
                .result(NoopResultHandler(handled));
        });
    }

    #[test]
    #[should_panic(
        expected = "section takes container slots 0..1, but the container only has 0 slots"
    )]
    fn result_placement_rejects_a_container_without_slot_zero() {
        let container = container(0);
        let mut b = MenuBuilder::new(None, 0);

        b.grid(1, |g| {
            let _ = g
                .place(Rect::cell(0, 0), container.clone())
                .result(NoopResultHandler(container.clone()));
            g.paint_all(ItemStack::empty());
        });
    }

    #[test]
    #[should_panic(expected = "two sections cover overlapping slots")]
    fn result_placement_rejects_a_normal_alias() {
        let container = container(1);
        let mut b = MenuBuilder::new(None, 0);

        b.grid(1, |g| {
            let _ = g.place(Rect::cell(0, 0), container.clone()).section();
            let _ = g
                .place(Rect::cell(1, 0), container.clone())
                .result(NoopResultHandler(container.clone()));
            g.paint_all(ItemStack::empty());
        });
    }

    #[test]
    #[should_panic(expected = "fake slots require exclusive backing storage")]
    fn raw_grid_slots_cannot_alias_result_backing_storage() {
        use crate::inventory::menu::kinds::BasicKind;

        let container = container(1);
        let slots: Vec<Box<dyn Slot>> = vec![
            Box::new(NormalSlot::new(container.clone(), 0)),
            Box::new(ResultSlot::new(NoopResultHandler(container.clone()))),
        ];
        let mut b = MenuBuilder::new(None, 0);
        b.grid(1, |g| {
            let _ = g.place_boxed_slots(Rect::cols(0..2).rows(0), slots);
            g.paint_all(ItemStack::empty());
        });

        let _ = b.build(BasicKind);
    }

    #[test]
    #[should_panic(expected = "overlaps another placement")]
    fn overlapping_placements_panic() {
        let mut b = MenuBuilder::new(None, 0);
        b.grid(1, |g| {
            let _ = g.place(Rect::cols(0..5).rows(0), container(5)).region();
            let _ = g.place(Rect::cols(4..9).rows(0), container(5)).region();
        });
    }

    #[test]
    #[should_panic(expected = "exceeds the 9x1 grid area")]
    fn out_of_bounds_placement_panics() {
        let mut b = MenuBuilder::new(None, 0);
        b.grid(1, |g| {
            let _ = g.place(Rect::cols(5..10).rows(0), container(5)).region();
        });
    }

    #[test]
    #[should_panic(expected = "not fully covered")]
    fn uncovered_cells_panic() {
        let mut b = MenuBuilder::new(None, 0);
        b.grid(1, |g| {
            let _ = g.place(Rect::cols(0..4).rows(0), container(4)).region();
        });
    }

    #[test]
    #[should_panic(expected = "not fully covered")]
    fn subgrid_must_cover_itself_despite_parent_paint() {
        let mut b = MenuBuilder::new(None, 0);
        b.grid(2, |g| {
            g.paint_all(ItemStack::empty());
            g.subgrid(Rect::cols(0..4).rows(0), |g| {
                let _ = g.place(Rect::cols(0..2).rows(0), container(2)).region();
            });
        });
    }

    #[test]
    #[should_panic(expected = "cannot mix rows() and cols()")]
    fn mixing_carve_axes_panics() {
        let mut b = MenuBuilder::new(None, 0);
        b.grid(2, |g| {
            g.carve_rows(1, |g| g.place(g.full(), container(9)).region());
            g.carve_cols(4, |g| g.place(g.full(), container(4)).region());
        });
    }

    #[test]
    #[should_panic(expected = "grid starts mid-row")]
    fn grid_after_partial_row_panics() {
        let mut b = MenuBuilder::new(None, 0);
        b.section(container(5), 5);
        b.grid(1, |g| {
            g.paint_all(ItemStack::empty());
        });
    }

    #[test]
    fn range_flavors_and_axis_orders_agree() {
        let mut b = MenuBuilder::new(None, 0);
        let (left, right) = b.grid(2, |g| {
            let left = g.place(Rect::cols(..=3).rows(..), container(8)).region();
            let right = g.place(Rect::rows(..).cols(4..), container(10)).region();
            (left, right)
        });
        assert_eq!(ranges(&left), vec![(0, 4), (9, 13)]);
        assert_eq!(ranges(&right), vec![(4, 9), (13, 18)]);
    }

    #[test]
    fn unbounded_ends_resolve_against_the_subgrid() {
        let mut b = MenuBuilder::new(None, 0);
        let inner = b.grid(2, |g| {
            g.paint_all(ItemStack::empty());
            g.subgrid(Rect::cols(1..5).rows(0), |g| {
                let inner = g.place(Rect::cols(2..).rows(..), container(2)).region();
                g.paint_all(ItemStack::empty());
                inner
            })
        });
        assert_eq!(ranges(&inner), vec![(3, 5)]);
    }

    #[test]
    #[should_panic(expected = "column range 3..3 is empty")]
    fn empty_range_panics_at_construction() {
        let _ = Rect::cols(3..3);
    }

    #[test]
    #[should_panic(expected = "exceeds the 9x1 grid area")]
    fn from_range_starting_past_the_edge_panics() {
        let mut b = MenuBuilder::new(None, 0);
        b.grid(1, |g| {
            let _ = g.place(Rect::cols(9..).rows(..), container(1)).region();
        });
    }

    #[test]
    #[should_panic(expected = "non-contiguous")]
    fn single_panics_on_multi_row_narrow_region() {
        let mut b = MenuBuilder::new(None, 0);
        b.grid(2, |g| {
            let region = g.place(Rect::cols(0..4).rows(..), container(8)).region();
            g.paint_all(ItemStack::empty());
            let _ = region.single();
        });
    }
}

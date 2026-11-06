//! Superclass chain, interface closure and override grouping for one class.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::model::{MethodDecl, TypeDecl};
use crate::source::SourceTree;

/// A supertype that could not be followed because it is not in the source
/// tree (JDK or library types).
pub struct ExternalSupertype {
    pub from: &'static TypeDecl,
    pub name: String,
}

/// Methods that are override-equivalent for the flattened class: one
/// declaration per type that declares it.
pub struct Slot {
    pub name: String,
    /// Declarations ordered most-derived first: classes in chain order, then
    /// interfaces in discovery order.
    pub decls: Vec<&'static MethodDecl>,
    pub is_static: bool,
}

/// Which declarations a call can see: classes from `min_class` up the chain
/// (none when `None`) plus the listed interfaces.
pub struct View {
    pub min_class: Option<usize>,
    pub interfaces: FxHashSet<&'static str>,
    /// The type making the call, which alone may see its private methods.
    pub caller: &'static str,
}

pub struct Hierarchy {
    pub target: &'static TypeDecl,
    /// Target first (when it is a class), then each superclass.
    pub classes: Vec<&'static TypeDecl>,
    /// Every reachable interface in discovery order (target first when it is
    /// an interface).
    pub interfaces: Vec<&'static TypeDecl>,
    pub external: Vec<ExternalSupertype>,
    pub slots: Vec<Slot>,
    /// Transitive super-interfaces of each interface.
    interface_supers: FxHashMap<&'static str, FxHashSet<&'static str>>,
    /// Interfaces reachable from `classes[k..]`, indexed by `k`.
    interfaces_from_class: Vec<FxHashSet<&'static str>>,
}

impl Hierarchy {
    pub fn build(source: &SourceTree, target: &'static TypeDecl) -> Self {
        let mut hierarchy = Self {
            target,
            classes: Vec::new(),
            interfaces: Vec::new(),
            external: Vec::new(),
            slots: Vec::new(),
            interface_supers: FxHashMap::default(),
            interfaces_from_class: Vec::new(),
        };

        if target.is_interface() {
            hierarchy.add_interface(source, target);
        } else {
            let mut current = Some(target);
            while let Some(class) = current {
                if hierarchy.classes.iter().any(|c| c.fqn == class.fqn) {
                    break;
                }
                hierarchy.classes.push(class);
                current = class.superclass.as_deref().and_then(|written| {
                    let resolved = source.resolve_type(class, written);
                    if resolved.is_none() {
                        hierarchy.external.push(ExternalSupertype {
                            from: class,
                            name: written.to_owned(),
                        });
                    }
                    resolved
                });
            }
            for k in (0..hierarchy.classes.len()).rev() {
                let class = hierarchy.classes[k];
                let mut reachable = hierarchy
                    .interfaces_from_class
                    .first()
                    .cloned()
                    .unwrap_or_default();
                for written in &class.interfaces {
                    match source.resolve_type(class, written) {
                        Some(interface) => {
                            hierarchy.add_interface(source, interface);
                            reachable.insert(interface.fqn.as_str());
                            reachable
                                .extend(hierarchy.interface_supers[interface.fqn.as_str()].iter());
                        }
                        None => hierarchy.external.push(ExternalSupertype {
                            from: class,
                            name: written.clone(),
                        }),
                    }
                }
                hierarchy.interfaces_from_class.insert(0, reachable);
            }
        }

        hierarchy.build_slots();
        hierarchy
    }

    fn add_interface(&mut self, source: &SourceTree, interface: &'static TypeDecl) {
        if self.interface_supers.contains_key(interface.fqn.as_str()) {
            return;
        }
        self.interface_supers
            .insert(&interface.fqn, FxHashSet::default());
        self.interfaces.push(interface);
        let mut supers = FxHashSet::default();
        for written in &interface.interfaces {
            match source.resolve_type(interface, written) {
                Some(parent) => {
                    self.add_interface(source, parent);
                    supers.insert(parent.fqn.as_str());
                    supers.extend(self.interface_supers[parent.fqn.as_str()].iter());
                }
                None => self.external.push(ExternalSupertype {
                    from: interface,
                    name: written.clone(),
                }),
            }
        }
        self.interface_supers.insert(&interface.fqn, supers);
    }

    /// Groups declarations into override slots, walking from the top of the
    /// chain down so each declaration joins the slot of what it overrides.
    fn build_slots(&mut self) {
        let types: Vec<&'static TypeDecl> = self
            .classes
            .iter()
            .rev()
            .chain(self.interfaces.iter())
            .copied()
            .collect();
        for ty in types {
            for method in &ty.methods {
                let existing = self.slots.iter_mut().find(|slot| {
                    slot.name == method.name
                        && slot.is_static == method.mods.is_static
                        && slot.decls.iter().any(|other| overrides(method, other))
                });
                match existing {
                    Some(slot) => slot.decls.push(method),
                    None => self.slots.push(Slot {
                        name: method.name.clone(),
                        decls: vec![method],
                        is_static: method.mods.is_static,
                    }),
                }
            }
        }
        let ranks: FxHashMap<&str, usize> = self
            .classes
            .iter()
            .chain(self.interfaces.iter())
            .enumerate()
            .map(|(rank, ty)| (ty.fqn.as_str(), rank))
            .collect();
        for slot in &mut self.slots {
            slot.decls.sort_by_key(|decl| ranks[decl.owner.as_str()]);
        }
    }

    /// The topmost class whose supertypes include `interface`.
    pub fn introduced_by(&self, interface: &TypeDecl) -> Option<&'static TypeDecl> {
        self.interfaces_from_class
            .iter()
            .rposition(|reachable| reachable.contains(interface.fqn.as_str()))
            .map(|k| self.classes[k])
    }

    /// Chain index of a class, or `None` for interfaces.
    pub fn class_index(&self, fqn: &str) -> Option<usize> {
        self.classes.iter().position(|c| c.fqn == fqn)
    }

    pub fn type_decl(&self, fqn: &str) -> Option<&'static TypeDecl> {
        self.classes
            .iter()
            .chain(self.interfaces.iter())
            .find(|t| t.fqn == fqn)
            .copied()
    }

    /// What the flattened class itself sees.
    pub fn target_view(&self) -> View {
        self.view_from(self.target)
    }

    /// What code declared in `ty` sees through `this`.
    pub fn view_from(&self, ty: &'static TypeDecl) -> View {
        match self.class_index(&ty.fqn) {
            Some(k) => self.class_view(k, &ty.fqn),
            None => self.interface_view(ty, &ty.fqn),
        }
    }

    /// What `super.m()` sees from the class at chain index `k`.
    pub fn super_view(&self, k: usize) -> View {
        self.class_view(k + 1, &self.classes[k].fqn)
    }

    /// What `I.super.m()` sees.
    pub fn interface_super_view(&self, interface: &'static TypeDecl, caller: &'static str) -> View {
        self.interface_view(interface, caller)
    }

    fn class_view(&self, k: usize, caller: &'static str) -> View {
        View {
            min_class: (k < self.classes.len()).then_some(k),
            interfaces: self
                .interfaces_from_class
                .get(k)
                .cloned()
                .unwrap_or_default(),
            caller,
        }
    }

    fn interface_view(&self, interface: &'static TypeDecl, caller: &'static str) -> View {
        let mut interfaces = self
            .interface_supers
            .get(interface.fqn.as_str())
            .cloned()
            .unwrap_or_default();
        interfaces.insert(&interface.fqn);
        View {
            min_class: None,
            interfaces,
            caller,
        }
    }

    pub fn visible_in(&self, decl: &MethodDecl, view: &View) -> bool {
        if decl.mods.is_private && decl.owner != view.caller {
            return false;
        }
        match self.class_index(&decl.owner) {
            Some(index) => view.min_class.is_some_and(|min| index >= min),
            None => view.interfaces.contains(decl.owner.as_str()),
        }
    }

    /// What runs for the flattened class. Private methods have a single
    /// declaration that only its owner can call, so that declaration is it.
    pub fn effective(&self, slot: &Slot) -> Implementation {
        match slot.decls.as_slice() {
            [only] if only.mods.is_private => Implementation::Concrete(only),
            _ => self.implementation(slot, &self.target_view()),
        }
    }

    /// The implementation a call through `view` runs.
    pub fn implementation(&self, slot: &Slot, view: &View) -> Implementation {
        let visible: Vec<&'static MethodDecl> = slot
            .decls
            .iter()
            .filter(|decl| self.visible_in(decl, view))
            .copied()
            .collect();
        if slot.is_static {
            return visible.first().map_or(Implementation::Missing, |decl| {
                Implementation::Concrete(decl)
            });
        }
        if let Some(class_impl) = visible
            .iter()
            .find(|decl| self.class_index(&decl.owner).is_some() && decl.is_concrete())
        {
            return Implementation::Concrete(class_impl);
        }
        // Java picks the most specific default among interfaces.
        let defaults: Vec<&'static MethodDecl> = visible
            .iter()
            .filter(|decl| self.class_index(&decl.owner).is_none() && decl.is_concrete())
            .copied()
            .collect();
        let most_specific: Vec<&'static MethodDecl> = defaults
            .iter()
            .filter(|candidate| {
                !defaults.iter().any(|other| {
                    other.owner != candidate.owner
                        && self.interface_supers[other.owner.as_str()]
                            .contains(candidate.owner.as_str())
                })
            })
            .copied()
            .collect();
        match most_specific.as_slice() {
            [single] => Implementation::Concrete(single),
            [] => visible.first().map_or(Implementation::Missing, |decl| {
                Implementation::Abstract(decl)
            }),
            conflicting => Implementation::Conflict(conflicting.to_vec()),
        }
    }
}

#[derive(Clone)]
pub enum Implementation {
    Concrete(&'static MethodDecl),
    /// Only abstract declarations are visible.
    Abstract(&'static MethodDecl),
    /// Unrelated interface defaults; javac would reject this, so it signals a
    /// resolution gap in the tool.
    Conflict(Vec<&'static MethodDecl>),
    Missing,
}

impl Implementation {
    pub const fn concrete(&self) -> Option<&'static MethodDecl> {
        match self {
            Self::Concrete(decl) => Some(decl),
            _ => None,
        }
    }
}

/// Whether `method` (declared lower or in an implemented interface) is
/// override-equivalent with `other` under Java's accessibility rules.
fn overrides(method: &MethodDecl, other: &MethodDecl) -> bool {
    if method.mods.is_private || other.mods.is_private || !method.same_params(other) {
        return false;
    }
    if method.mods.is_static {
        // Static methods hide rather than override; grouping them keeps the
        // hiding declaration next to the hidden one.
        return true;
    }
    if other.mods.is_package_private() || method.mods.is_package_private() {
        return method.file.package == other.file.package;
    }
    true
}

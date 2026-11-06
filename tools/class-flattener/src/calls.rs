//! Finds `super.m()`, `I.super.m()`, `this.m()`/`m()` and constructor
//! invocations inside a body and resolves them against the flattened
//! hierarchy.
//!
//! Overloads are chosen by arity first, then by lightweight argument typing
//! (literals, locals, parameters, fields, casts, `new`, and `this` calls).
//! When that is not enough the call is reported as ambiguous instead of
//! guessed.

use std::ptr;

use tree_sitter::Node;

use crate::hierarchy::{Hierarchy, Implementation, View};
use crate::model::{JavaFile, MethodDecl, Param, TypeDecl, TypeKind};
use crate::source::{SourceTree, base_type_text, erase};

#[derive(Clone, Copy)]
pub enum CallKind {
    Super,
    /// `Name.super.m()`, with the qualifier node.
    InterfaceSuper(Node<'static>),
    /// Unqualified `m()` or `this.m()`.
    This,
}

pub struct CallSite {
    pub node: Node<'static>,
    pub kind: CallKind,
    pub name: String,
    pub args: Vec<Node<'static>>,
}

pub enum CallOutcome {
    Super(Implementation),
    /// A `this` call whose runtime target is overridden below the caller.
    Dispatch(Implementation),
    /// A `this` call that runs what the caller sees.
    Direct,
    Ambiguous(Vec<usize>),
    /// Not a member of the flattened hierarchy (outer class, static import,
    /// or a supertype outside the source tree).
    Unresolved,
}

pub enum CtorOutcome {
    Resolved(&'static MethodDecl),
    /// A record's implicit canonical constructor.
    ImplicitCanonical(&'static TypeDecl),
    NotFound,
    /// The superclass declares no constructor.
    ImplicitDefault(&'static TypeDecl),
    Ambiguous(Vec<&'static MethodDecl>),
    /// The superclass is outside the source tree.
    External,
}

pub struct CtorCall {
    /// `None` for an implicit `super()`.
    pub node: Option<Node<'static>>,
    pub is_super: bool,
    pub outcome: CtorOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Only,
    First,
    Middle,
    Last,
}

/// Where a super call sits in its body, which decides whether a Steel port
/// can model the override as a pre/post hook or needs to replace it.
#[derive(Debug, Clone, Copy)]
pub struct CallShape {
    pub position: Position,
    pub conditional: bool,
    pub in_lambda: bool,
    pub after_early_return: bool,
}

impl CallShape {
    pub fn describe(self) -> String {
        let mut parts = vec![match self.position {
            Position::Only => "only statement",
            Position::First => "first",
            Position::Middle => "middle",
            Position::Last => "last",
        }];
        if self.conditional {
            parts.push("conditional");
        }
        if self.after_early_return {
            parts.push("after early return");
        }
        if self.in_lambda {
            parts.push("in lambda");
        }
        parts.join(", ")
    }
}

pub fn collect_calls(file: &JavaFile, body: Node<'static>) -> Vec<CallSite> {
    let mut out = Vec::new();
    visit_own_code(body, &mut |node| {
        if node.kind() != "method_invocation" {
            return;
        }
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let object = node.child_by_field_name("object");
        let mut cursor = node.walk();
        let has_qualified_super = node.children(&mut cursor).any(|c| c.kind() == "super");
        let kind = match object {
            None => CallKind::This,
            Some(object) if object.kind() == "this" => CallKind::This,
            Some(object) if object.kind() == "super" => CallKind::Super,
            Some(object) if has_qualified_super => CallKind::InterfaceSuper(object),
            Some(_) => return,
        };
        let args = node
            .child_by_field_name("arguments")
            .map_or_default(|list| {
                let mut cursor = list.walk();
                list.named_children(&mut cursor)
                    .filter(|c| !is_comment(*c))
                    .collect()
            });
        out.push(CallSite {
            node,
            kind,
            name: file.text(name).to_owned(),
            args,
        });
    });
    out
}

/// Visits nodes that run with the body's `this`, skipping anonymous and
/// local class bodies.
fn visit_own_code(node: Node<'static>, visit: &mut dyn FnMut(Node<'static>)) {
    visit(node);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if is_foreign_scope(child) {
            continue;
        }
        visit_own_code(child, visit);
    }
}

fn is_foreign_scope(node: Node) -> bool {
    matches!(
        node.kind(),
        "class_body"
            | "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
    )
}

fn is_comment(node: Node) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment")
}

pub fn call_shape(call: Node<'static>, body: Node<'static>) -> CallShape {
    let mut conditional = false;
    let mut in_lambda = false;
    let mut child = call;
    while let Some(parent) = child.parent() {
        if parent.id() == body.id() {
            break;
        }
        let field = field_of(parent, child);
        match parent.kind() {
            "if_statement" | "ternary_expression" | "while_statement" | "for_statement" => {
                conditional |= !matches!(field, Some("condition" | "init"));
            }
            "enhanced_for_statement" | "switch_expression" | "switch_statement" => {
                conditional |= field == Some("body");
            }
            "catch_clause" => conditional = true,
            "binary_expression" => {
                let operator = parent.child_by_field_name("operator").map(|o| o.kind());
                conditional |= matches!(operator, Some("&&" | "||")) && field == Some("right");
            }
            "lambda_expression" => in_lambda = true,
            _ => {}
        }
        child = parent;
    }

    let statements: Vec<Node<'static>> = {
        let mut cursor = body.walk();
        body.named_children(&mut cursor)
            .filter(|c| !is_comment(*c))
            .collect()
    };
    let index = statements
        .iter()
        .position(|s| s.id() == child.id())
        .unwrap_or(0);
    let position = match (statements.len(), index) {
        (1, _) => Position::Only,
        (_, 0) => Position::First,
        (len, i) if i + 1 == len => Position::Last,
        _ => Position::Middle,
    };
    let after_early_return = statements[..index].iter().any(|s| contains_exit(*s));
    CallShape {
        position,
        conditional,
        in_lambda,
        after_early_return,
    }
}

fn field_of(parent: Node<'static>, child: Node<'static>) -> Option<&'static str> {
    let mut cursor = parent.walk();
    for (index, candidate) in parent.children(&mut cursor).enumerate() {
        if candidate.id() == child.id() {
            return u32::try_from(index)
                .ok()
                .and_then(|i| parent.field_name_for_child(i));
        }
    }
    None
}

fn contains_exit(node: Node) -> bool {
    if matches!(node.kind(), "return_statement" | "throw_statement") {
        return true;
    }
    if is_foreign_scope(node) || node.kind() == "lambda_expression" {
        return false;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).any(contains_exit)
}

/// An inferred argument type: erased simple name plus what is needed to
/// resolve it for subtype checks.
struct ArgType {
    erased: String,
    written: String,
    ctx: Option<&'static TypeDecl>,
}

impl ArgType {
    fn primitive(name: &str) -> Self {
        Self {
            erased: name.to_owned(),
            written: name.to_owned(),
            ctx: None,
        }
    }
}

#[derive(PartialEq, Eq)]
enum Compat {
    Exact,
    Applicable,
    No,
}

pub struct Resolver<'a> {
    pub source: &'a SourceTree,
    pub hierarchy: &'a Hierarchy,
}

impl Resolver<'_> {
    pub fn resolve_call(&self, decl: &'static MethodDecl, call: &CallSite) -> CallOutcome {
        let Some(owner) = self.hierarchy.type_decl(&decl.owner) else {
            return CallOutcome::Unresolved;
        };
        let view = match call.kind {
            CallKind::Super => match self.hierarchy.class_index(&owner.fqn) {
                Some(k) => self.hierarchy.super_view(k),
                None => return CallOutcome::Unresolved,
            },
            CallKind::InterfaceSuper(qualifier) => {
                let written = owner.file.text(qualifier);
                let Some(interface) = self
                    .source
                    .resolve_type(owner, written)
                    .and_then(|i| self.hierarchy.type_decl(&i.fqn))
                else {
                    return CallOutcome::Unresolved;
                };
                self.hierarchy.interface_super_view(interface, &owner.fqn)
            }
            CallKind::This => self.hierarchy.view_from(owner),
        };

        let slot = match self.choose_slot(&view, &call.name, &call.args, decl, owner) {
            Ok(slot) => slot,
            Err(candidates) if candidates.is_empty() => return CallOutcome::Unresolved,
            Err(candidates) => return CallOutcome::Ambiguous(candidates),
        };
        let implementation = self
            .hierarchy
            .implementation(&self.hierarchy.slots[slot], &view);
        match call.kind {
            CallKind::Super | CallKind::InterfaceSuper(_) => CallOutcome::Super(implementation),
            CallKind::This => {
                let slot_ref = &self.hierarchy.slots[slot];
                if slot_ref.is_static || slot_ref.decls.iter().all(|d| d.mods.is_private) {
                    return CallOutcome::Direct;
                }
                let actual = self.hierarchy.effective(slot_ref);
                if same_implementation(&implementation, &actual) {
                    CallOutcome::Direct
                } else {
                    CallOutcome::Dispatch(actual)
                }
            }
        }
    }

    /// Resolves the explicit or implicit `this(...)`/`super(...)` of a
    /// constructor.
    pub fn resolve_constructor_call(&self, ctor: &'static MethodDecl) -> Option<CtorCall> {
        let owner = self.hierarchy.type_decl(&ctor.owner)?;
        let body = ctor.body?;
        let mut cursor = body.walk();
        let explicit = body
            .named_children(&mut cursor)
            .find(|s| s.kind() == "explicit_constructor_invocation");
        let is_super = explicit
            .and_then(|e| e.child_by_field_name("constructor"))
            .is_none_or(|c| c.kind() == "super");

        if !is_super {
            let candidates: Vec<&'static MethodDecl> = owner
                .constructors
                .iter()
                .filter(|c| !ptr::eq(*c, ctor))
                .collect();
            let args = explicit.map_or_default(argument_nodes);
            let mut outcome = self.choose_constructor(&candidates, &args, ctor, owner);
            if matches!(outcome, CtorOutcome::NotFound)
                && owner.kind == TypeKind::Record
                && args.len() == owner.record_components.len()
            {
                outcome = CtorOutcome::ImplicitCanonical(owner);
            }
            return Some(CtorCall {
                node: explicit,
                is_super,
                outcome,
            });
        }

        // Enums, records and classes without `extends` chain to JDK types.
        if !matches!(owner.kind, TypeKind::Class) || owner.superclass.is_none() {
            return None;
        }
        let k = self.hierarchy.class_index(&owner.fqn)?;
        let Some(superclass) = self.hierarchy.classes.get(k + 1) else {
            return Some(CtorCall {
                node: explicit,
                is_super,
                outcome: CtorOutcome::External,
            });
        };
        let args = explicit.map_or_default(argument_nodes);
        let outcome = if superclass.constructors.is_empty() {
            CtorOutcome::ImplicitDefault(superclass)
        } else {
            let candidates: Vec<&'static MethodDecl> = superclass.constructors.iter().collect();
            self.choose_constructor(&candidates, &args, ctor, owner)
        };
        Some(CtorCall {
            node: explicit,
            is_super,
            outcome,
        })
    }

    fn choose_constructor(
        &self,
        candidates: &[&'static MethodDecl],
        args: &[Node<'static>],
        ctx_decl: &MethodDecl,
        ctx_owner: &'static TypeDecl,
    ) -> CtorOutcome {
        let picked = self.pick_overload(
            candidates.iter().map(|c| (*c, *c)).collect(),
            args,
            ctx_decl,
            ctx_owner,
        );
        match picked {
            Ok(decl) => CtorOutcome::Resolved(decl),
            Err(remaining) if remaining.is_empty() => CtorOutcome::NotFound,
            Err(remaining) => CtorOutcome::Ambiguous(remaining),
        }
    }

    fn choose_slot(
        &self,
        view: &View,
        name: &str,
        args: &[Node<'static>],
        ctx_decl: &MethodDecl,
        ctx_owner: &'static TypeDecl,
    ) -> Result<usize, Vec<usize>> {
        let candidates: Vec<(usize, &'static MethodDecl)> = self
            .hierarchy
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.name == name)
            .filter_map(|(index, slot)| {
                slot.decls
                    .iter()
                    .find(|d| self.hierarchy.visible_in(d, view))
                    .map(|d| (index, *d))
            })
            .collect();
        self.pick_overload(candidates, args, ctx_decl, ctx_owner)
    }

    /// Java-style overload selection over `(key, declaration)` candidates.
    fn pick_overload<K: Copy>(
        &self,
        candidates: Vec<(K, &'static MethodDecl)>,
        args: &[Node<'static>],
        ctx_decl: &MethodDecl,
        ctx_owner: &'static TypeDecl,
    ) -> Result<K, Vec<K>> {
        let by_arity: Vec<(K, &'static MethodDecl)> = candidates
            .into_iter()
            .filter(|(_, d)| arity_matches(&d.params, args.len()))
            .collect();
        if let [(key, _)] = by_arity.as_slice() {
            return Ok(*key);
        }
        if by_arity.is_empty() {
            return Err(Vec::new());
        }

        let arg_types: Vec<Option<ArgType>> = args
            .iter()
            .map(|a| self.infer(*a, ctx_decl, ctx_owner))
            .collect();
        let scored: Vec<(K, &'static MethodDecl, bool)> = by_arity
            .iter()
            .filter_map(|(key, decl)| {
                let mut exact = true;
                for (i, arg) in arg_types.iter().enumerate() {
                    let param = param_for(&decl.params, i)?;
                    match self.compat(arg.as_ref(), param) {
                        Compat::No => return None,
                        Compat::Applicable => exact = false,
                        Compat::Exact => {}
                    }
                }
                Some((*key, *decl, exact))
            })
            .collect();
        // Java prefers non-varargs applicability (phases 1-2 before 3).
        let fixed: Vec<_> = scored
            .iter()
            .filter(|(_, d, _)| !d.params.last().is_some_and(|p| p.varargs))
            .collect();
        let pool: Vec<_> = if fixed.is_empty() {
            scored.iter().collect()
        } else {
            fixed
        };
        if let [(key, _, _)] = pool.as_slice() {
            return Ok(*key);
        }
        let exact: Vec<_> = pool.iter().filter(|(_, _, e)| *e).collect();
        if let [(key, _, _)] = exact.as_slice() {
            return Ok(*key);
        }
        let most_specific: Vec<_> = pool
            .iter()
            .filter(|(_, candidate, _)| {
                pool.iter().all(|(_, other, _)| {
                    ptr::eq(*candidate, *other) || self.more_specific(candidate, other)
                })
            })
            .collect();
        if let [(key, _, _)] = most_specific.as_slice() {
            return Ok(*key);
        }
        Err(pool.iter().map(|(key, _, _)| *key).collect())
    }

    fn more_specific(&self, a: &'static MethodDecl, b: &'static MethodDecl) -> bool {
        a.params.iter().zip(&b.params).all(|(pa, pb)| {
            let arg = ArgType {
                erased: pa.ty.clone(),
                written: pa.raw_ty.clone(),
                ctx: self.hierarchy.type_decl(&a.owner),
            };
            self.compat(Some(&arg), pb) != Compat::No
        })
    }

    fn compat(&self, arg: Option<&ArgType>, param: &Param) -> Compat {
        let Some(arg) = arg else {
            return Compat::Applicable;
        };
        let p = param.ty.as_str();
        let a = arg.erased.as_str();
        if p == "*" || p.starts_with('*') {
            return Compat::Applicable;
        }
        if a == p {
            return Compat::Exact;
        }
        if a == "null" {
            return if is_primitive(p) {
                Compat::No
            } else {
                Compat::Applicable
            };
        }
        match (is_primitive(a), is_primitive(p)) {
            (true, true) => {
                if widens(a, p) {
                    Compat::Applicable
                } else {
                    Compat::No
                }
            }
            (true, false) => {
                if boxed(a) == Some(p)
                    || matches!(p, "Object" | "Number" | "Comparable" | "Serializable")
                {
                    Compat::Applicable
                } else {
                    Compat::No
                }
            }
            (false, true) => match unboxed(a) {
                Some(prim) if prim == p || widens(prim, p) => Compat::Applicable,
                _ => Compat::No,
            },
            (false, false) => {
                if a.ends_with("[]") != p.ends_with("[]") {
                    return if p == "Object" {
                        Compat::Applicable
                    } else {
                        Compat::No
                    };
                }
                let Some(resolved) = arg
                    .ctx
                    .and_then(|ctx| self.source.resolve_type(ctx, &arg.written))
                else {
                    return Compat::Applicable;
                };
                if self
                    .source
                    .ancestry(resolved)
                    .names
                    .contains(p.trim_end_matches("[]"))
                {
                    Compat::Applicable
                } else {
                    Compat::No
                }
            }
        }
    }

    /// Like [`Self::infer_raw`], with type variables replaced by their bound.
    fn infer(
        &self,
        node: Node<'static>,
        decl: &MethodDecl,
        owner: &'static TypeDecl,
    ) -> Option<ArgType> {
        let mut value = self.infer_raw(node, decl, owner)?;
        if value.erased == "*"
            && let Some(bound) = decl.bound_of(&value.written)
        {
            value.erased = bound.rsplit('.').next().unwrap_or(bound).to_owned();
            value.written = bound.to_owned();
        }
        Some(value)
    }

    fn infer_raw(
        &self,
        node: Node<'static>,
        decl: &MethodDecl,
        owner: &'static TypeDecl,
    ) -> Option<ArgType> {
        let file = decl.file;
        let text = file.text(node);
        match node.kind() {
            "parenthesized_expression" => self.infer(node.named_child(0)?, decl, owner),
            "decimal_integer_literal"
            | "hex_integer_literal"
            | "octal_integer_literal"
            | "binary_integer_literal" => Some(ArgType::primitive(if text.ends_with(['l', 'L']) {
                "long"
            } else {
                "int"
            })),
            "decimal_floating_point_literal" | "hex_floating_point_literal" => {
                Some(ArgType::primitive(if text.ends_with(['f', 'F']) {
                    "float"
                } else {
                    "double"
                }))
            }
            "string_literal" | "text_block" => Some(ArgType::primitive("String")),
            "character_literal" => Some(ArgType::primitive("char")),
            "true" | "false" => Some(ArgType::primitive("boolean")),
            "null_literal" => Some(ArgType::primitive("null")),
            "class_literal" => Some(ArgType::primitive("Class")),
            "this" => Some(ArgType {
                erased: owner.name.clone(),
                written: owner.name.clone(),
                ctx: Some(owner),
            }),
            "cast_expression" | "object_creation_expression" => {
                let ty = node.child_by_field_name("type")?;
                Some(ArgType {
                    erased: erase(file, ty, &owner.type_params),
                    written: base_type_text(file, ty),
                    ctx: Some(owner),
                })
            }
            "identifier" => self.infer_name(text, node, decl, owner),
            "field_access" => {
                let object = node.child_by_field_name("object")?;
                let field = file.text(node.child_by_field_name("field")?);
                if object.kind() == "this" {
                    return self.field_type(field, owner);
                }
                let receiver = self.receiver_type(object, decl, owner)?;
                self.member_field(receiver, field)
            }
            "unary_expression" => {
                let operator = node.child_by_field_name("operator")?;
                if operator.kind() == "!" {
                    Some(ArgType::primitive("boolean"))
                } else {
                    self.infer(node.child_by_field_name("operand")?, decl, owner)
                }
            }
            "binary_expression" => {
                let operator = node.child_by_field_name("operator")?.kind();
                if matches!(
                    operator,
                    "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||" | "instanceof"
                ) {
                    return Some(ArgType::primitive("boolean"));
                }
                let left = self.infer(node.child_by_field_name("left")?, decl, owner)?;
                let right = self.infer(node.child_by_field_name("right")?, decl, owner)?;
                if operator == "+" && (left.erased == "String" || right.erased == "String") {
                    return Some(ArgType::primitive("String"));
                }
                promote(&left.erased, &right.erased).map(ArgType::primitive)
            }
            "ternary_expression" => {
                self.infer(node.child_by_field_name("consequence")?, decl, owner)
            }
            "method_invocation" => {
                let calls = collect_calls(file, node);
                let Some(call) = calls.first().filter(|c| c.node.id() == node.id()) else {
                    return self.infer_foreign_call(node, decl, owner);
                };
                if !matches!(call.kind, CallKind::This) {
                    return None;
                }
                let view = self.hierarchy.view_from(owner);
                let slot = self
                    .choose_slot(&view, &call.name, &call.args, decl, owner)
                    .ok()?;
                let target = self.hierarchy.slots[slot].decls.first()?;
                Some(ArgType {
                    erased: target.return_type.clone(),
                    written: target.raw_return_type.clone(),
                    ctx: self.hierarchy.type_decl(&target.owner),
                })
            }
            _ => None,
        }
    }

    /// Calls in `decl` whose receiver is another object of a type that
    /// extends `Entity`: `(receiver type, method name, line)`.
    pub fn entity_receiver_calls(&self, decl: &'static MethodDecl) -> Vec<(String, String, usize)> {
        let Some(body) = decl.body else {
            return Vec::new();
        };
        let Some(owner) = self.source.load_fqn(&decl.owner) else {
            return Vec::new();
        };
        let mut found = Vec::new();
        visit_own_code(body, &mut |node| {
            if node.kind() != "method_invocation" {
                return;
            }
            let (Some(object), Some(name)) = (
                node.child_by_field_name("object"),
                node.child_by_field_name("name"),
            ) else {
                return;
            };
            if matches!(object.kind(), "this" | "super") {
                return;
            }
            // Static calls name a type rather than evaluate to a value.
            let Some(receiver) = self
                .infer(object, decl, owner)
                .and_then(|value| self.source.resolve_type(value.ctx?, &value.written))
            else {
                return;
            };
            if self.source.ancestry(receiver).names.contains("Entity") {
                found.push((
                    receiver.display_name().to_owned(),
                    decl.file.text(name).to_owned(),
                    node.start_position().row + 1,
                ));
            }
        });
        found
    }

    /// Return type of a call on another receiver (`entity.position()`,
    /// `Vec3.atCenterOf(pos)`), using the same overload selection.
    fn infer_foreign_call(
        &self,
        node: Node<'static>,
        decl: &MethodDecl,
        owner: &'static TypeDecl,
    ) -> Option<ArgType> {
        let file = decl.file;
        let receiver = self.receiver_type(node.child_by_field_name("object")?, decl, owner)?;
        let name = file.text(node.child_by_field_name("name")?);
        let args = argument_nodes(node);
        let ancestry = &self.source.ancestry(receiver).types;
        if args.is_empty()
            && let Some((ty, component)) = ancestry.iter().find_map(|ty| {
                ty.record_components
                    .iter()
                    .find(|c| c.name == name)
                    .map(|c| (*ty, c))
            })
        {
            return Some(ArgType {
                erased: component.ty.clone(),
                written: component.raw_ty.clone(),
                ctx: Some(ty),
            });
        }
        // Most-derived first, skipping declarations they override.
        let mut candidates: Vec<(&'static MethodDecl, &'static MethodDecl)> = Vec::new();
        for ty in ancestry {
            for method in ty.methods.iter().filter(|m| m.name == name) {
                if !candidates.iter().any(|(seen, _)| seen.same_params(method)) {
                    candidates.push((method, method));
                }
            }
        }
        let target = self.pick_overload(candidates, &args, decl, owner).ok()?;
        Some(ArgType {
            erased: target.return_type.clone(),
            written: target.raw_return_type.clone(),
            ctx: self.source.load_fqn(&target.owner),
        })
        .filter(|t| t.erased != "*")
    }

    /// The type an expression evaluates to, or the type it names for static
    /// access (`EntityTypeTags.FALL_DAMAGE_IMMUNE`).
    fn receiver_type(
        &self,
        object: Node<'static>,
        decl: &MethodDecl,
        owner: &'static TypeDecl,
    ) -> Option<&'static TypeDecl> {
        if let Some(value) = self.infer(object, decl, owner) {
            return value
                .ctx
                .and_then(|ctx| self.source.resolve_type(ctx, &value.written));
        }
        if matches!(
            object.kind(),
            "identifier" | "field_access" | "scoped_identifier"
        ) {
            return self.source.resolve_type(owner, decl.file.text(object));
        }
        None
    }

    fn member_field(&self, receiver: &'static TypeDecl, name: &str) -> Option<ArgType> {
        self.source.ancestry(receiver).types.iter().find_map(|ty| {
            let field = ty
                .fields
                .iter()
                .find(|f| f.names.iter().any(|n| n == name))
                .map(|f| (f.ty.as_str(), f.raw_ty.as_str()));
            let component = ty
                .record_components
                .iter()
                .find(|c| c.name == name)
                .map(|c| (c.ty.as_str(), c.raw_ty.as_str()));
            field.or(component).map(|(erased, written)| ArgType {
                erased: erased.to_owned(),
                written: written.to_owned(),
                ctx: Some(*ty),
            })
        })
    }

    fn infer_name(
        &self,
        name: &str,
        usage: Node<'static>,
        decl: &MethodDecl,
        owner: &'static TypeDecl,
    ) -> Option<ArgType> {
        let file = decl.file;
        let mut best: Option<(usize, LocalDecl)> = None;
        if let Some(body) = decl.body {
            visit_own_code(body, &mut |node| {
                if node.start_byte() >= usage.start_byte() {
                    return;
                }
                let Some(declared_type) = local_declaration_type(file, node, name) else {
                    return;
                };
                let in_scope = declaration_scope(node)
                    .is_some_and(|scope| scope.byte_range().contains(&usage.start_byte()));
                if in_scope && best.is_none_or(|(start, _)| node.start_byte() > start) {
                    best = Some((node.start_byte(), declared_type));
                }
            });
        }
        if let Some((_, declared)) = best {
            let LocalDecl::Typed(ty) = declared else {
                return None;
            };
            return Some(ArgType {
                erased: erase(file, ty, &owner.type_params),
                written: base_type_text(file, ty),
                ctx: Some(owner),
            });
        }
        if let Some(param) = decl.params.iter().find(|p| p.name == name) {
            return Some(ArgType {
                erased: param.ty.clone(),
                written: param.raw_ty.clone(),
                ctx: Some(owner),
            });
        }
        self.field_type(name, owner)
    }

    fn field_type(&self, name: &str, owner: &'static TypeDecl) -> Option<ArgType> {
        let start = self
            .hierarchy
            .class_index(&owner.fqn)
            .unwrap_or(self.hierarchy.classes.len());
        self.hierarchy.classes[start.min(self.hierarchy.classes.len())..]
            .iter()
            .chain(self.hierarchy.interfaces.iter())
            .find_map(|ty| {
                ty.fields
                    .iter()
                    .find(|f| f.names.iter().any(|n| n == name))
                    .map(|f| ArgType {
                        erased: f.ty.clone(),
                        written: f.raw_ty.clone(),
                        ctx: Some(*ty),
                    })
            })
    }
}

/// A local variable declaration found while inferring a name.
#[derive(Clone, Copy)]
enum LocalDecl {
    Typed(Node<'static>),
    /// Lambda parameters and `var`: they shadow outer names but have no
    /// written type.
    Untyped,
}

/// If `node` declares a local named `name`, returns how it is typed.
fn local_declaration_type(file: &JavaFile, node: Node<'static>, name: &str) -> Option<LocalDecl> {
    let typed = |ty: Option<Node<'static>>| match ty.filter(|t| file.text(*t) != "var") {
        Some(ty) => LocalDecl::Typed(ty),
        None => LocalDecl::Untyped,
    };
    match node.kind() {
        "local_variable_declaration" | "resource" => {
            let mut cursor = node.walk();
            let declares = node
                .children_by_field_name("declarator", &mut cursor)
                .filter_map(|d| d.child_by_field_name("name"))
                .any(|n| file.text(n) == name)
                || node
                    .child_by_field_name("name")
                    .is_some_and(|n| file.text(n) == name);
            declares.then(|| typed(node.child_by_field_name("type")))
        }
        "enhanced_for_statement" | "catch_formal_parameter" | "instanceof_expression" => {
            let declared = node.child_by_field_name("name")?;
            (file.text(declared) == name).then(|| {
                let ty = match node.kind() {
                    "instanceof_expression" => node.child_by_field_name("right"),
                    "catch_formal_parameter" => {
                        let mut cursor = node.walk();
                        node.named_children(&mut cursor)
                            .find(|c| c.kind() == "catch_type")
                            .and_then(|c| c.named_child(0))
                    }
                    _ => node.child_by_field_name("type"),
                };
                typed(ty)
            })
        }
        "type_pattern" => {
            let mut cursor = node.walk();
            let children: Vec<Node<'static>> = node.named_children(&mut cursor).collect();
            let declared = children.iter().find(|c| c.kind() == "identifier")?;
            (file.text(*declared) == name).then(|| typed(children.first().copied()))
        }
        "lambda_expression" => {
            let params = node.child_by_field_name("parameters")?;
            let declares = if params.kind() == "identifier" {
                file.text(params) == name
            } else {
                let mut cursor = params.walk();
                params.named_children(&mut cursor).any(|p| {
                    let ident = if p.kind() == "identifier" {
                        Some(p)
                    } else {
                        p.child_by_field_name("name")
                    };
                    ident.is_some_and(|i| file.text(i) == name)
                })
            };
            declares.then_some(LocalDecl::Untyped)
        }
        _ => None,
    }
}

/// Region a local declared by `node` is visible in. Pattern variables are
/// flow scoped (`if (!(x instanceof T t)) return; t.use();`), approximated by
/// the block around their statement.
fn declaration_scope(node: Node<'static>) -> Option<Node<'static>> {
    if !matches!(node.kind(), "instanceof_expression" | "type_pattern") {
        return node.parent();
    }
    let mut current = node;
    while let Some(parent) = current.parent() {
        if matches!(
            parent.kind(),
            "block"
                | "constructor_body"
                | "switch_block_statement_group"
                | "switch_rule"
                | "lambda_expression"
        ) {
            return Some(parent);
        }
        current = parent;
    }
    None
}

fn argument_nodes(invocation: Node<'static>) -> Vec<Node<'static>> {
    invocation
        .child_by_field_name("arguments")
        .map_or_default(|list| {
            let mut cursor = list.walk();
            list.named_children(&mut cursor)
                .filter(|c| !is_comment(*c))
                .collect()
        })
}

const fn arity_matches(params: &[Param], args: usize) -> bool {
    match params.last() {
        Some(last) if last.varargs => args + 1 >= params.len(),
        _ => params.len() == args,
    }
}

fn param_for(params: &[Param], index: usize) -> Option<&Param> {
    match params.last() {
        Some(last) if last.varargs && index + 1 >= params.len() => Some(last),
        _ => params.get(index),
    }
}

fn same_implementation(a: &Implementation, b: &Implementation) -> bool {
    match (a, b) {
        (Implementation::Concrete(x), Implementation::Concrete(y))
        | (Implementation::Abstract(x), Implementation::Abstract(y)) => ptr::eq(*x, *y),
        (Implementation::Missing, Implementation::Missing) => true,
        _ => false,
    }
}

fn is_primitive(ty: &str) -> bool {
    matches!(
        ty,
        "boolean" | "byte" | "short" | "char" | "int" | "long" | "float" | "double"
    )
}

fn widens(from: &str, to: &str) -> bool {
    let targets: &[&str] = match from {
        "byte" => &["short", "int", "long", "float", "double"],
        "short" | "char" => &["int", "long", "float", "double"],
        "int" => &["long", "float", "double"],
        "long" => &["float", "double"],
        "float" => &["double"],
        _ => &[],
    };
    targets.contains(&to)
}

fn boxed(primitive: &str) -> Option<&'static str> {
    Some(match primitive {
        "boolean" => "Boolean",
        "byte" => "Byte",
        "short" => "Short",
        "char" => "Character",
        "int" => "Integer",
        "long" => "Long",
        "float" => "Float",
        "double" => "Double",
        _ => return None,
    })
}

fn unboxed(reference: &str) -> Option<&'static str> {
    Some(match reference {
        "Boolean" => "boolean",
        "Byte" => "byte",
        "Short" => "short",
        "Character" => "char",
        "Integer" => "int",
        "Long" => "long",
        "Float" => "float",
        "Double" => "double",
        _ => return None,
    })
}

fn promote(a: &str, b: &str) -> Option<&'static str> {
    const ORDER: [&str; 4] = ["int", "long", "float", "double"];
    let rank = |t: &str| match t {
        "byte" | "short" | "char" | "int" => Some(0),
        "long" => Some(1),
        "float" => Some(2),
        "double" => Some(3),
        _ => None,
    };
    Some(ORDER[rank(a)?.max(rank(b)?)])
}

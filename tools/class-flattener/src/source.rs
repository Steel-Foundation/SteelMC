//! Lazily parses the decompiled source tree and resolves type names the way
//! `javac` would for the cases that occur in vanilla: member types, single-type
//! imports, same-package types, on-demand imports and qualified names.

use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

use crate::model::{FieldDecl, JavaFile, MethodDecl, Modifiers, Param, TypeDecl, TypeKind};

pub struct SourceTree {
    root: PathBuf,
    parser: RefCell<Parser>,
    /// Paths already loaded (or found missing), so each file is parsed once.
    loaded: RefCell<FxHashSet<PathBuf>>,
    types: RefCell<FxHashMap<String, &'static TypeDecl>>,
    ancestry: RefCell<FxHashMap<String, &'static Ancestry>>,
}

pub struct Ancestry {
    pub types: Vec<&'static TypeDecl>,
    pub names: FxHashSet<String>,
}

impl SourceTree {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        if !root.is_dir() {
            return Err(format!(
                "source root {} does not exist; run tools/update-minecraft-src.sh",
                root.display()
            ));
        }
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .map_err(|e| format!("failed to load the Java grammar: {e}"))?;
        Ok(Self {
            root,
            parser: RefCell::new(parser),
            loaded: RefCell::new(FxHashSet::default()),
            types: RefCell::new(FxHashMap::default()),
            ancestry: RefCell::new(FxHashMap::default()),
        })
    }

    /// Finds a class from a CLI argument: a simple name (`Chicken`), a nested
    /// name (`Fox.FoxBreedGoal`, `Fox$FoxBreedGoal`) or a fully qualified name.
    pub fn find_class(&self, query: &str) -> Result<&'static TypeDecl, String> {
        let query = query.replace('$', ".");
        if let Some(found) = self.load_fqn(&query) {
            return Ok(found);
        }

        let segments: Vec<&str> = query.split('.').collect();
        let mut files = Vec::new();
        collect_java_files(&self.root, &mut |path| {
            if path.file_stem().is_some_and(|stem| stem == segments[0]) {
                files.push(path.to_path_buf());
            }
        })?;
        for path in &files {
            self.load_file(path)?;
        }
        let mut matches = self.types_named(&query);

        if matches.is_empty() {
            // Member types are not discoverable from file names.
            files.clear();
            collect_java_files(&self.root, &mut |path| files.push(path.to_path_buf()))?;
            for path in &files {
                self.load_file(path)?;
            }
            matches = self.types_named(&query);
        }

        match matches.as_slice() {
            [] => Err(format!(
                "no class named `{query}` under {}",
                self.root.display()
            )),
            [single] => Ok(single),
            _ => {
                let mut names: Vec<&str> = matches.iter().map(|t| t.fqn.as_str()).collect();
                names.sort_unstable();
                Err(format!(
                    "`{query}` is ambiguous; pass one of:\n  {}",
                    names.join("\n  ")
                ))
            }
        }
    }

    fn types_named(&self, display_name: &str) -> Vec<&'static TypeDecl> {
        self.types
            .borrow()
            .values()
            .filter(|t| t.display_name() == display_name || t.name == display_name)
            .copied()
            .collect()
    }

    /// Loads `package.Outer.Inner` by trying each prefix as a file path.
    pub fn load_fqn(&self, fqn: &str) -> Option<&'static TypeDecl> {
        if let Some(found) = self.types.borrow().get(fqn) {
            return Some(found);
        }
        let segments: Vec<&str> = fqn.split('.').collect();
        for len in (1..=segments.len()).rev() {
            let mut path = self.root.join(segments[..len].join("/"));
            path.set_extension("java");
            if path.is_file() {
                if let Err(err) = self.load_file(&path) {
                    eprintln!("warning: {err}");
                }
                return self.types.borrow().get(fqn).copied();
            }
        }
        None
    }

    /// Resolves a type name as written inside `ctx` (generic arguments already
    /// stripped). Returns `None` for JDK/library types outside the source root.
    pub fn resolve_type(&self, ctx: &'static TypeDecl, written: &str) -> Option<&'static TypeDecl> {
        let mut segments = written.split('.');
        let first = segments.next()?;
        let mut resolved = self.resolve_simple(ctx, first);
        if resolved.is_none() && written.contains('.') {
            return self.load_fqn(written);
        }
        for segment in segments {
            let outer = resolved?;
            resolved = self.member_type(outer, segment);
        }
        resolved
    }

    fn resolve_simple(&self, ctx: &'static TypeDecl, name: &str) -> Option<&'static TypeDecl> {
        // Member types of the context and its enclosing types shadow imports.
        let mut scope = Some(ctx);
        while let Some(current) = scope {
            if current.name == name {
                return Some(current);
            }
            if let Some(member) = self.member_type(current, name) {
                return Some(member);
            }
            scope = current
                .outer
                .as_deref()
                .and_then(|outer| self.load_fqn(outer));
        }

        let file = ctx.file;
        if let Some(import) = file
            .single_imports
            .iter()
            .find(|import| import.rsplit('.').next() == Some(name))
        {
            return self.load_fqn(import);
        }
        let same_package = if file.package.is_empty() {
            name.to_owned()
        } else {
            format!("{}.{name}", file.package)
        };
        if let Some(found) = self.load_fqn(&same_package) {
            return Some(found);
        }
        file.wildcard_imports
            .iter()
            .find_map(|prefix| self.load_fqn(&format!("{prefix}.{name}")))
    }

    fn member_type(&self, outer: &'static TypeDecl, name: &str) -> Option<&'static TypeDecl> {
        if !outer.nested.iter().any(|nested| nested == name) {
            return None;
        }
        self.types
            .borrow()
            .get(&format!("{}.{name}", outer.fqn))
            .copied()
    }

    /// `ty` followed by every resolvable supertype (breadth first), plus the
    /// simple names of all supertypes including unresolvable ones.
    pub fn ancestry(&self, ty: &'static TypeDecl) -> &'static Ancestry {
        if let Some(ancestry) = self.ancestry.borrow().get(&ty.fqn) {
            return ancestry;
        }
        let mut ancestry = Ancestry {
            types: Vec::new(),
            names: FxHashSet::from_iter(["Object".to_owned()]),
        };
        let mut pending = VecDeque::from([ty]);
        while let Some(current) = pending.pop_front() {
            if ancestry.types.iter().any(|t| t.fqn == current.fqn) {
                continue;
            }
            ancestry.types.push(current);
            ancestry.names.insert(current.name.clone());
            for written in current.superclass.iter().chain(&current.interfaces) {
                match self.resolve_type(current, written) {
                    Some(resolved) => pending.push_back(resolved),
                    None => {
                        ancestry
                            .names
                            .insert(written.rsplit('.').next().unwrap_or(written).to_owned());
                    }
                }
            }
        }
        let ancestry: &'static Ancestry = Box::leak(Box::new(ancestry));
        self.ancestry.borrow_mut().insert(ty.fqn.clone(), ancestry);
        ancestry
    }

    fn load_file(&self, path: &Path) -> Result<(), String> {
        if !self.loaded.borrow_mut().insert(path.to_path_buf()) {
            return Ok(());
        }
        let src = fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let tree = self
            .parser
            .borrow_mut()
            .parse(&src, None)
            .ok_or_else(|| format!("failed to parse {}", path.display()))?;
        let rel_path = path
            .strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        let mut file = JavaFile {
            rel_path,
            src,
            tree,
            package: String::new(),
            single_imports: Vec::new(),
            wildcard_imports: Vec::new(),
            parse_error_lines: Vec::new(),
        };
        read_header(&mut file);
        let file: &'static JavaFile = Box::leak(Box::new(file));

        let mut decls = Vec::new();
        let root = file.tree.root_node();
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            extract_type(file, child, None, &[], &[], &mut decls);
        }
        let mut types = self.types.borrow_mut();
        for decl in decls {
            let decl: &'static TypeDecl = Box::leak(Box::new(decl));
            types.insert(decl.fqn.clone(), decl);
        }
        Ok(())
    }
}

fn collect_java_files(dir: &Path, visit: &mut dyn FnMut(&Path)) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("failed to read {}: {e}", dir.display()))?
            .path();
        if path.is_dir() {
            collect_java_files(&path, visit)?;
        } else if path.extension().is_some_and(|ext| ext == "java") {
            visit(&path);
        }
    }
    Ok(())
}

fn read_header(file: &mut JavaFile) {
    let root = file.tree.root_node();
    let mut cursor = root.walk();
    let mut package = String::new();
    let mut single = Vec::new();
    let mut wildcard = Vec::new();
    for child in root.named_children(&mut cursor) {
        match child.kind() {
            "package_declaration" => {
                if let Some(name) = last_named_child(child) {
                    file.text(name).clone_into(&mut package);
                }
            }
            "import_declaration" => {
                let mut inner = child.walk();
                let children: Vec<Node> = child.children(&mut inner).collect();
                if children.iter().any(|c| c.kind() == "static") {
                    continue;
                }
                let Some(name) = children
                    .iter()
                    .find(|c| matches!(c.kind(), "scoped_identifier" | "identifier"))
                else {
                    continue;
                };
                let name = file.text(*name).to_owned();
                if children.iter().any(|c| c.kind() == "asterisk") {
                    wildcard.push(name);
                } else {
                    single.push(name);
                }
            }
            _ => {}
        }
    }
    let mut errors = Vec::new();
    collect_error_lines(root, &mut errors);
    errors.dedup();
    file.package = package;
    file.single_imports = single;
    file.wildcard_imports = wildcard;
    file.parse_error_lines = errors;
}

fn collect_error_lines(node: Node, out: &mut Vec<usize>) {
    if node.is_error() || node.is_missing() {
        out.push(node.start_position().row + 1);
        return;
    }
    if !node.has_error() {
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_error_lines(child, out);
    }
}

const TYPE_DECLARATIONS: [&str; 5] = [
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration",
];

fn extract_type(
    file: &'static JavaFile,
    node: Node<'static>,
    outer: Option<&str>,
    outer_type_params: &[String],
    outer_type_bounds: &[(String, String)],
    out: &mut Vec<TypeDecl>,
) {
    let kind = match node.kind() {
        "class_declaration" => TypeKind::Class,
        "interface_declaration" => TypeKind::Interface,
        "enum_declaration" => TypeKind::Enum,
        "record_declaration" => TypeKind::Record,
        "annotation_type_declaration" => TypeKind::Annotation,
        _ => return,
    };
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = file.text(name_node).to_owned();
    let fqn = match (outer, file.package.is_empty()) {
        (Some(outer), _) => format!("{outer}.{name}"),
        (None, true) => name.clone(),
        (None, false) => format!("{}.{name}", file.package),
    };

    let mut type_params = outer_type_params.to_vec();
    type_params.extend(type_parameter_names(file, node));
    let mut type_bounds = outer_type_bounds.to_vec();
    type_bounds.extend(type_parameter_bounds(file, node));

    let superclass = node
        .child_by_field_name("superclass")
        .and_then(|s| s.named_child(0))
        .map(|t| base_type_text(file, t));
    let interface_list = match kind {
        TypeKind::Interface => find_child(node, "extends_interfaces"),
        _ => node.child_by_field_name("interfaces"),
    };
    let interfaces = interface_list
        .and_then(|list| find_child(list, "type_list"))
        .map_or_default(|list| {
            let mut cursor = list.walk();
            list.named_children(&mut cursor)
                .map(|t| base_type_text(file, t))
                .collect()
        });

    let mut decl = TypeDecl {
        fqn,
        name,
        kind,
        file,
        outer: outer.map(str::to_owned),
        type_params: type_params.clone(),
        superclass,
        interfaces,
        fields: Vec::new(),
        methods: Vec::new(),
        constructors: Vec::new(),
        nested: Vec::new(),
        record_components: node
            .child_by_field_name("parameters")
            .filter(|_| kind == TypeKind::Record)
            .map_or_default(|list| parameters(file, list, &type_params)),
    };
    if let Some(body) = node.child_by_field_name("body") {
        add_members(file, body, &mut decl, (&type_params, &type_bounds), out);
    }
    out.push(decl);
}

fn add_members(
    file: &'static JavaFile,
    body: Node<'static>,
    decl: &mut TypeDecl,
    (type_params, type_bounds): (&[String], &[(String, String)]),
    out: &mut Vec<TypeDecl>,
) {
    let fqn = decl.fqn.clone();
    let display = decl.display_name().to_owned();
    let in_interface = decl.is_interface();
    let mut members: Vec<Node<'static>> = Vec::new();
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if child.kind() == "enum_body_declarations" {
            let mut inner = child.walk();
            members.extend(child.named_children(&mut inner));
        } else {
            members.push(child);
        }
    }

    for member in members {
        match member.kind() {
            "field_declaration" | "constant_declaration" => {
                decl.fields
                    .push(field_decl(file, member, type_params, in_interface));
            }
            "method_declaration" => decl.methods.push(method_decl(
                file,
                member,
                &fqn,
                &display,
                (type_params, type_bounds),
                in_interface,
            )),
            "constructor_declaration" | "compact_constructor_declaration" => {
                let mut ctor = method_decl(
                    file,
                    member,
                    &fqn,
                    &display,
                    (type_params, type_bounds),
                    false,
                );
                if member.kind() == "compact_constructor_declaration" {
                    ctor.params.clone_from(&decl.record_components);
                }
                decl.constructors.push(ctor);
            }
            kind if TYPE_DECLARATIONS.contains(&kind) => {
                if let Some(nested_name) = member.child_by_field_name("name") {
                    decl.nested.push(file.text(nested_name).to_owned());
                }
                extract_type(file, member, Some(&fqn), type_params, type_bounds, out);
            }
            _ => {}
        }
    }
}

fn find_child<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|c| c.kind() == kind)
}

fn type_parameter_names(file: &JavaFile, node: Node) -> Vec<String> {
    let Some(params) = node.child_by_field_name("type_parameters") else {
        return Vec::new();
    };
    let mut cursor = params.walk();
    params
        .named_children(&mut cursor)
        .filter(|p| p.kind() == "type_parameter")
        .filter_map(|p| {
            let mut inner = p.walk();
            p.named_children(&mut inner)
                .find(|c| c.kind() == "type_identifier")
                .map(|c| file.text(c).to_owned())
        })
        .collect()
}

/// `(T, Bound)` for each `T extends Bound & ...` (first bound, which is the
/// erasure).
fn type_parameter_bounds(file: &JavaFile, node: Node) -> Vec<(String, String)> {
    let Some(params) = node.child_by_field_name("type_parameters") else {
        return Vec::new();
    };
    let mut cursor = params.walk();
    params
        .named_children(&mut cursor)
        .filter(|p| p.kind() == "type_parameter")
        .filter_map(|p| {
            let mut inner = p.walk();
            let children: Vec<Node> = p.named_children(&mut inner).collect();
            let name = children.iter().find(|c| c.kind() == "type_identifier")?;
            let bound = children
                .iter()
                .find(|c| c.kind() == "type_bound")?
                .named_child(0)?;
            Some((file.text(*name).to_owned(), base_type_text(file, bound)))
        })
        .collect()
}

fn modifiers(file: &JavaFile, node: Node, in_interface: bool) -> Modifiers {
    let mut mods = Modifiers::default();
    if let Some(list) = find_child(node, "modifiers") {
        let mut cursor = list.walk();
        for child in list.children(&mut cursor) {
            match file.text(child) {
                "static" => mods.is_static = true,
                "abstract" => mods.is_abstract = true,
                "private" => mods.is_private = true,
                "public" => mods.is_public = true,
                "protected" => mods.is_protected = true,
                "default" => mods.is_default = true,
                _ => {}
            }
        }
    }
    if in_interface && !mods.is_private {
        mods.is_public = true;
    }
    mods
}

fn field_decl(
    file: &'static JavaFile,
    node: Node<'static>,
    type_params: &[String],
    in_interface: bool,
) -> FieldDecl {
    let ty_node = node.child_by_field_name("type");
    let mut cursor = node.walk();
    let names = node
        .children_by_field_name("declarator", &mut cursor)
        .filter_map(|d| d.child_by_field_name("name"))
        .map(|n| file.text(n).to_owned())
        .collect();
    FieldDecl {
        node,
        names,
        ty: ty_node.map_or_default(|t| erase(file, t, type_params)),
        raw_ty: ty_node.map_or_default(|t| base_type_text(file, t)),
        is_static: in_interface || modifiers(file, node, false).is_static,
    }
}

fn method_decl(
    file: &'static JavaFile,
    node: Node<'static>,
    owner: &str,
    owner_display: &str,
    (class_type_params, class_type_bounds): (&[String], &[(String, String)]),
    in_interface: bool,
) -> MethodDecl {
    let mut type_params = class_type_params.to_vec();
    type_params.extend(type_parameter_names(file, node));
    let mut type_bounds = class_type_bounds.to_vec();
    type_bounds.extend(type_parameter_bounds(file, node));

    let name_node = node.child_by_field_name("name");
    let name = name_node.map_or_default(|n| file.text(n).to_owned());
    let mut mods = modifiers(file, node, in_interface);
    let body = node.child_by_field_name("body");
    if in_interface && body.is_none() && !mods.is_static {
        mods.is_abstract = true;
    }

    let params = node
        .child_by_field_name("parameters")
        .map_or_default(|list| parameters(file, list, &type_params));

    MethodDecl {
        owner: owner.to_owned(),
        owner_display: owner_display.to_owned(),
        name,
        params,
        return_type: node
            .child_by_field_name("type")
            .map_or_default(|t| erase(file, t, &type_params)),
        raw_return_type: node
            .child_by_field_name("type")
            .map_or_default(|t| base_type_text(file, t)),
        mods,
        type_bounds,
        node,
        body,
        line: name_node.unwrap_or(node).start_position().row + 1,
        file,
    }
}

fn last_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).last()
}

fn parameters(file: &JavaFile, list: Node, type_params: &[String]) -> Vec<Param> {
    let mut params = Vec::new();
    let mut cursor = list.walk();
    for param in list.named_children(&mut cursor) {
        match param.kind() {
            "formal_parameter" => {
                let ty = param.child_by_field_name("type");
                let mut erased = ty.map_or_default(|t| erase(file, t, type_params));
                // C-style `Type name[]`.
                if let Some(dims) = param.child_by_field_name("dimensions") {
                    erased.push_str(&"[]".repeat(file.text(dims).matches('[').count()));
                }
                params.push(Param {
                    name: param
                        .child_by_field_name("name")
                        .map_or_default(|n| file.text(n).to_owned()),
                    ty: erased,
                    raw_ty: ty.map_or_default(|t| base_type_text(file, t)),
                    varargs: false,
                });
            }
            "spread_parameter" => {
                let mut inner = param.walk();
                let children: Vec<Node> = param.named_children(&mut inner).collect();
                let ty = children
                    .iter()
                    .find(|c| !matches!(c.kind(), "modifiers" | "variable_declarator"));
                let name = children
                    .iter()
                    .find(|c| c.kind() == "variable_declarator")
                    .and_then(|d| d.child_by_field_name("name"));
                params.push(Param {
                    name: name.map_or_default(|n| file.text(n).to_owned()),
                    ty: ty.map_or_default(|t| format!("{}[]", erase(file, *t, type_params))),
                    raw_ty: ty.map_or_default(|t| base_type_text(file, *t)),
                    varargs: true,
                });
            }
            _ => {}
        }
    }
    params
}

/// Erased simple name used for override matching: `List<T>` -> `List`,
/// `Entity.RemovalReason` -> `RemovalReason`, a type variable -> `*`.
pub fn erase(file: &JavaFile, node: Node, type_params: &[String]) -> String {
    match node.kind() {
        "type_identifier" => {
            let name = file.text(node);
            if type_params.iter().any(|p| p == name) {
                "*".to_owned()
            } else {
                name.to_owned()
            }
        }
        "scoped_type_identifier" => {
            last_named_child(node).map_or_default(|last| file.text(last).to_owned())
        }
        "generic_type" | "annotated_type" => {
            let inner = if node.kind() == "annotated_type" {
                last_named_child(node)
            } else {
                node.named_child(0)
            };
            inner.map_or_default(|inner| erase(file, inner, type_params))
        }
        "array_type" => {
            let element = node
                .child_by_field_name("element")
                .map_or_default(|e| erase(file, e, type_params));
            let dims = node
                .child_by_field_name("dimensions")
                .map_or(1, |d| file.text(d).matches('[').count());
            // Arrays of type variables still erase to the bound's array type,
            // which is not known here.
            let element = if element == "*" {
                "*".to_owned()
            } else {
                element
            };
            format!("{element}{}", "[]".repeat(dims))
        }
        _ => file.text(node).split_whitespace().collect(),
    }
}

/// Type as written with generic arguments and annotations removed, kept
/// qualified so it can be resolved (`Entity.RemovalReason`).
pub fn base_type_text(file: &JavaFile, node: Node) -> String {
    match node.kind() {
        "generic_type" => node
            .named_child(0)
            .map_or_default(|inner| base_type_text(file, inner)),
        "annotated_type" => {
            last_named_child(node).map_or_default(|inner| base_type_text(file, inner))
        }
        "array_type" => node
            .child_by_field_name("element")
            .map_or_default(|inner| base_type_text(file, inner)),
        _ => file.text(node).split_whitespace().collect(),
    }
}

//! Declarations extracted from parsed Java source.

use tree_sitter::Node;

/// A parsed `.java` file. Files are leaked for the lifetime of the process so
/// syntax nodes can be shared freely; the tool is a short-lived CLI.
pub struct JavaFile {
    /// Path relative to the source root, using `/` separators.
    pub rel_path: String,
    pub src: String,
    pub tree: tree_sitter::Tree,
    pub package: String,
    /// Fully qualified single-type imports (`import a.b.C;`).
    pub single_imports: Vec<String>,
    /// Prefixes of on-demand imports (`import a.b.*;` stores `a.b`).
    pub wildcard_imports: Vec<String>,
    /// 1-based lines that contain tree-sitter parse errors.
    pub parse_error_lines: Vec<usize>,
}

impl JavaFile {
    pub fn text(&self, node: Node) -> &str {
        &self.src[node.byte_range()]
    }

    pub fn file_name(&self) -> &str {
        self.rel_path.rsplit('/').next().unwrap_or(&self.rel_path)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
}

pub struct TypeDecl {
    /// `package.Outer.Inner`.
    pub fqn: String,
    pub name: String,
    pub kind: TypeKind,
    pub file: &'static JavaFile,
    pub outer: Option<String>,
    pub type_params: Vec<String>,
    /// Supertype names as written, generic arguments stripped (`Foo.Bar`, `Animal`).
    pub superclass: Option<String>,
    pub interfaces: Vec<String>,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<MethodDecl>,
    pub constructors: Vec<MethodDecl>,
    /// Simple names of member types.
    pub nested: Vec<String>,
    /// Record components, which also act as fields and accessor methods.
    pub record_components: Vec<Param>,
}

impl TypeDecl {
    /// Name without the package, e.g. `Fox.FoxBreedGoal`.
    pub fn display_name(&self) -> &str {
        if self.file.package.is_empty() {
            &self.fqn
        } else {
            &self.fqn[self.file.package.len() + 1..]
        }
    }

    pub const fn is_interface(&self) -> bool {
        matches!(self.kind, TypeKind::Interface | TypeKind::Annotation)
    }
}

pub struct FieldDecl {
    pub node: Node<'static>,
    pub names: Vec<String>,
    /// Erased simple type name.
    pub ty: String,
    /// Type as written without generic arguments, for resolving.
    pub raw_ty: String,
    pub is_static: bool,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Modifiers {
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_private: bool,
    pub is_public: bool,
    pub is_protected: bool,
    pub is_default: bool,
}

impl Modifiers {
    pub const fn is_package_private(self) -> bool {
        !(self.is_private || self.is_public || self.is_protected)
    }
}

#[derive(Clone)]
pub struct Param {
    pub name: String,
    /// Erased simple type name; `*` for a type variable.
    pub ty: String,
    /// Type as written without generic arguments, for resolving.
    pub raw_ty: String,
    pub varargs: bool,
}

pub struct MethodDecl {
    /// Fully qualified name of the declaring type.
    pub owner: String,
    pub owner_display: String,
    /// Method name, or the type name for constructors.
    pub name: String,
    pub params: Vec<Param>,
    /// Erased simple return type; empty for constructors.
    pub return_type: String,
    /// Return type as written without generic arguments, for resolving.
    pub raw_return_type: String,
    pub mods: Modifiers,
    /// Bounds of the method's and its class's type variables.
    pub type_bounds: Vec<(String, String)>,
    pub node: Node<'static>,
    pub body: Option<Node<'static>>,
    /// 1-based line of the method name.
    pub line: usize,
    pub file: &'static JavaFile,
}

impl MethodDecl {
    /// The bound a type variable named `name` stands for here.
    pub fn bound_of(&self, name: &str) -> Option<&str> {
        self.type_bounds
            .iter()
            .rev()
            .find(|(param, _)| param == name)
            .map(|(_, bound)| bound.as_str())
    }

    /// `name(Type, Type)`.
    pub fn signature(&self) -> String {
        format!("{}({})", self.name, self.param_list())
    }

    fn param_list(&self) -> String {
        let params: Vec<String> = self
            .params
            .iter()
            .map(|p| {
                let ty = if p.ty == "*" {
                    p.raw_ty.as_str()
                } else {
                    p.ty.as_str()
                };
                if p.varargs {
                    format!("{}...", ty.trim_end_matches("[]"))
                } else {
                    ty.to_owned()
                }
            })
            .collect();
        params.join(", ")
    }

    pub const fn is_constructor(&self) -> bool {
        self.return_type.is_empty()
    }

    /// `Owner.name(Type)`, or `Owner(Type)` for constructors.
    pub fn qualified_signature(&self) -> String {
        if self.is_constructor() {
            format!("{}({})", self.owner_display, self.param_list())
        } else {
            format!("{}.{}", self.owner_display, self.signature())
        }
    }

    /// Whether the body has no statements.
    pub fn is_empty(&self) -> bool {
        self.body.is_some_and(|body| {
            let mut cursor = body.walk();
            body.named_children(&mut cursor)
                .all(|c| matches!(c.kind(), "line_comment" | "block_comment"))
        })
    }

    /// `File.java:line`.
    pub fn location(&self) -> String {
        format!("{}:{}", self.file.file_name(), self.line)
    }

    pub const fn is_concrete(&self) -> bool {
        self.body.is_some() && !self.mods.is_abstract
    }

    /// Whether `self` and `other` have override-equivalent parameter lists.
    /// Type variables (`*`) match anything because generic supertypes are not
    /// substituted.
    pub fn same_params(&self, other: &MethodDecl) -> bool {
        self.params.len() == other.params.len()
            && self
                .params
                .iter()
                .zip(&other.params)
                .all(|(a, b)| a.ty == b.ty || a.ty == "*" || b.ty == "*")
    }
}

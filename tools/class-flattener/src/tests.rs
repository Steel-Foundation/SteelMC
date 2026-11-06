//! Resolution rules checked against small Java fixtures. Each case is one
//! where a naive name lookup gives a different answer than javac.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{env, fs, process, ptr};

use crate::calls::{CallOutcome, CtorOutcome, Resolver, collect_calls};
use crate::hierarchy::{Hierarchy, Implementation};
use crate::model::MethodDecl;
use crate::source::SourceTree;

struct Fixture {
    root: PathBuf,
    source: SourceTree,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = env::temp_dir().join(format!(
            "class-flattener-test-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        for (path, contents) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().expect("fixture paths have a parent"))
                .expect("create fixture dir");
            fs::write(path, contents).expect("write fixture");
        }
        let source = SourceTree::new(root.clone()).expect("fixture root exists");
        Self { root, source }
    }

    fn hierarchy(&self, class: &str) -> Hierarchy {
        Hierarchy::build(
            &self.source,
            self.source.find_class(class).expect("class exists"),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn method(h: &Hierarchy, owner: &str, name: &str) -> &'static MethodDecl {
    h.type_decl(owner)
        .and_then(|t| t.methods.iter().find(|m| m.name == name))
        .unwrap_or_else(|| panic!("{owner}.{name} exists"))
}

/// Outcomes of calls named `callee` inside `owner.name`.
fn outcomes(
    h: &Hierarchy,
    source: &SourceTree,
    owner: &str,
    name: &str,
    callee: &str,
) -> Vec<CallOutcome> {
    let decl = method(h, owner, name);
    let resolver = Resolver {
        source,
        hierarchy: h,
    };
    collect_calls(decl.file, decl.body.expect("has body"))
        .iter()
        .filter(|c| c.name == callee)
        .map(|c| resolver.resolve_call(decl, c))
        .collect()
}

fn concrete_owner(outcome: &CallOutcome) -> String {
    let implementation = match outcome {
        CallOutcome::Super(i) | CallOutcome::Dispatch(i) => i,
        CallOutcome::Direct => return "direct".to_owned(),
        CallOutcome::Ambiguous(_) => return "ambiguous".to_owned(),
        CallOutcome::Unresolved => return "unresolved".to_owned(),
    };
    match implementation {
        Implementation::Concrete(d) => d.owner_display.clone(),
        _ => "not concrete".to_owned(),
    }
}

#[test]
fn super_skips_classes_that_do_not_override() {
    let f = Fixture::new(&[
        (
            "p/A.java",
            "package p; public class A { public void tick() {} }",
        ),
        ("p/B.java", "package p; public class B extends A { }"),
        (
            "p/C.java",
            "package p; public class C extends B { public void tick() { super.tick(); } }",
        ),
    ]);
    let h = f.hierarchy("p.C");
    let found = outcomes(&h, &f.source, "p.C", "tick", "tick");
    assert_eq!(concrete_owner(&found[0]), "A");
}

#[test]
fn inherited_code_dispatches_to_the_flattened_override() {
    let f = Fixture::new(&[
        (
            "p/A.java",
            "package p; public class A { public void tick() { this.step(); helper(); } \
             protected void step() {} private void helper() {} }",
        ),
        (
            "p/B.java",
            "package p; public class B extends A { protected void step() {} void helper() {} }",
        ),
    ]);
    let h = f.hierarchy("p.B");
    assert_eq!(
        concrete_owner(&outcomes(&h, &f.source, "p.A", "tick", "step")[0]),
        "B"
    );
    // A private method is not overridden by a same-signature subclass method.
    assert!(matches!(
        outcomes(&h, &f.source, "p.A", "tick", "helper")[0],
        CallOutcome::Direct
    ));
}

#[test]
fn package_private_methods_are_not_overridden_from_another_package() {
    let f = Fixture::new(&[
        (
            "a/A.java",
            "package a; public class A { public void run() { hook(); } void hook() {} }",
        ),
        (
            "b/B.java",
            "package b; public class B extends a.A { void hook() {} }",
        ),
    ]);
    let h = f.hierarchy("b.B");
    assert!(matches!(
        outcomes(&h, &f.source, "a.A", "run", "hook")[0],
        CallOutcome::Direct
    ));
    assert_eq!(h.slots.iter().filter(|s| s.name == "hook").count(), 2);
}

#[test]
fn overloads_are_chosen_by_argument_types() {
    let f = Fixture::new(&[
        (
            "p/A.java",
            "package p; public class A { public void put(int v) {} public void put(String v) {} \
             public void put(Base v) {} public Object level() { return null; } }",
        ),
        ("p/Base.java", "package p; public class Base {}"),
        ("p/Sub.java", "package p; public class Sub extends Base {}"),
        (
            "p/B.java",
            "package p; public class B extends A { \
             public void put(String v) { super.put(1); super.put(v); } \
             public void put(Base v) { if (this.level() instanceof Sub sub && v != null) { super.put(sub); } } }",
        ),
    ]);
    let h = f.hierarchy("p.B");
    let b = h.type_decl("p.B").expect("B");
    let resolver = Resolver {
        source: &f.source,
        hierarchy: &h,
    };
    let params_of = |decl: &'static MethodDecl| -> Vec<String> {
        collect_calls(decl.file, decl.body.expect("body"))
            .iter()
            .filter(|c| c.name == "put")
            .map(|c| match resolver.resolve_call(decl, c) {
                CallOutcome::Super(Implementation::Concrete(target)) => target.signature(),
                _ => "unresolved".to_owned(),
            })
            .collect()
    };
    assert_eq!(params_of(&b.methods[0]), ["put(int)", "put(String)"]);
    // `sub` is a pattern variable declared in the condition, used in the body.
    assert_eq!(params_of(&b.methods[1]), ["put(Base)"]);
}

#[test]
fn class_methods_beat_defaults_and_the_most_specific_default_wins() {
    let f = Fixture::new(&[
        (
            "p/I.java",
            "package p; public interface I { default void a() {} default void b() {} }",
        ),
        (
            "p/J.java",
            "package p; public interface J extends I { default void b() {} }",
        ),
        (
            "p/A.java",
            "package p; public class A { public void a() {} }",
        ),
        (
            "p/B.java",
            "package p; public class B extends A implements J { \
             public void call() { a(); b(); I.super.b(); } }",
        ),
    ]);
    let h = f.hierarchy("p.B");
    let effective = |name: &str| {
        let slot = h.slots.iter().find(|s| s.name == name).expect("slot");
        h.effective(slot)
            .concrete()
            .map(|d| d.owner_display.clone())
    };
    assert_eq!(effective("a").as_deref(), Some("A"));
    assert_eq!(effective("b").as_deref(), Some("J"));
}

#[test]
fn qualified_interface_super_targets_that_interface() {
    let f = Fixture::new(&[
        (
            "p/I.java",
            "package p; public interface I { default void b() {} }",
        ),
        (
            "p/A.java",
            "package p; public class A { public void b() {} }",
        ),
        (
            "p/B.java",
            "package p; public class B extends A implements I { public void b() { I.super.b(); } }",
        ),
    ]);
    let h = f.hierarchy("p.B");
    assert_eq!(
        concrete_owner(&outcomes(&h, &f.source, "p.B", "b", "b")[0]),
        "I"
    );
}

#[test]
fn constructor_chains_follow_this_and_implicit_super() {
    let f = Fixture::new(&[
        (
            "p/A.java",
            "package p; public class A { public A() {} public A(int x) {} }",
        ),
        (
            "p/B.java",
            "package p; public class B extends A { public B() { this(\"x\"); } public B(String s) { } }",
        ),
    ]);
    let h = f.hierarchy("p.B");
    let resolver = Resolver {
        source: &f.source,
        hierarchy: &h,
    };
    let b = h.type_decl("p.B").expect("B");
    let first = resolver
        .resolve_constructor_call(&b.constructors[0])
        .expect("explicit this");
    assert!(!first.is_super);
    assert!(
        matches!(first.outcome, CtorOutcome::Resolved(target) if ptr::eq(target, &raw const b.constructors[1]))
    );
    let second = resolver
        .resolve_constructor_call(&b.constructors[1])
        .expect("implicit super");
    assert!(second.node.is_none());
    assert!(matches!(second.outcome, CtorOutcome::Resolved(target) if target.params.is_empty()));
}

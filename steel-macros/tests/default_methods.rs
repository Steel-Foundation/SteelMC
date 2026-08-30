//! Integration coverage for explicit trait default calls.

#![deny(unfulfilled_lint_expectations, unused_mut)]

use steel_macros::default_methods;

#[default_methods]
trait Example {
    #[expect(unused_variables, reason = "the default ignores the argument")]
    fn borrowed<'a>(&self, other: &'a str) -> &str {
        "default"
    }

    fn changed(&self, mut value: i32) -> i32 {
        value += 1;
        value
    }

    fn ignored(&self, _value: i32) {}

    fn generic<T, const N: usize>(&self, value: T) -> (T, usize)
    where
        Self: Sized,
    {
        (value, N)
    }
}

struct Override;

impl Example for Override {
    fn changed(&self, value: i32) -> i32 {
        ExampleDefaults::changed(self, value) * 2
    }
}

#[test]
fn defaults_are_callable_for_concrete_and_erased_implementors() {
    let value = Override;
    assert_eq!(value.changed(2), 6);
    value.ignored(1);
    assert_eq!(ExampleDefaults::borrowed(&value, "other"), "default");
    assert_eq!(
        ExampleDefaults::generic::<_, 4>(&value, "value"),
        ("value", 4)
    );

    let erased: &dyn Example = &value;
    assert_eq!(ExampleDefaults::borrowed(erased, "other"), "default");
}

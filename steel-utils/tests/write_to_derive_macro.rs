//! Tests for the macro deriving `WriteTo` implementations.

use steel_macros::WriteTo;
use steel_utils::serial::WriteTo;

#[test]
fn enum_dispatch() {
    #[derive(WriteTo)]
    #[write(as = Dispatched)]
    enum X<'a, 'b> {
        A(&'a i32, #[write(as = VarLong)] i64, &'b bool),
        B {
            // To test variable name collisions with the generated implementation
            #[write(as = VarInt)]
            writer: i32,

            y: &'b i16,
        },
        C,
    }

    let mut buffer = Vec::new();

    let a = X::A(&3, 4, &true);
    a.write(&mut buffer)
        .expect("should have written without any errors");
    assert_eq!(buffer, [0x00, 0x00, 0x00, 0x00, 0x03, 0x04, 0x01]);
    buffer.clear();

    let b = X::B {
        writer: 127,
        y: &0x1234,
    };
    b.write(&mut buffer)
        .expect("should have written without any errors");
    assert_eq!(buffer, [0x01, 0x7F, 0x12, 0x34]);
    buffer.clear();

    X::C.write(&mut buffer)
        .expect("should have written without any errors");
    assert_eq!(buffer, [0x02]);
}

/// `where` clauses, lifetime parameters and borrowed payloads must survive the
/// derive. `split_for_impl()` returns the clause as a third element, and it was
/// being discarded, so the generated impl only compiled when every bound was
/// written inline on the generic parameter.
#[test]
fn generics_and_where_clause_are_preserved() {
    #[derive(WriteTo)]
    #[write(as = Dispatched)]
    enum Packet<'a, T>
    where
        T: WriteTo,
    {
        Data(&'a T),
        Text(#[write(as = Prefixed)] &'a str),
        Empty,
    }

    #[derive(WriteTo)]
    struct Holder<T>
    where
        T: WriteTo,
    {
        value: T,
    }

    let mut buffer = Vec::new();

    let value = 7u8;
    Packet::Data(&value)
        .write(&mut buffer)
        .expect("should have written without any errors");
    assert_eq!(buffer, [0x00, 0x07]);
    buffer.clear();

    let text: Packet<'_, u8> = Packet::Text("hi");
    text.write(&mut buffer)
        .expect("should have written without any errors");
    assert_eq!(buffer, [0x01, 0x02, b'h', b'i']);
    buffer.clear();

    let empty: Packet<'_, u8> = Packet::Empty;
    empty
        .write(&mut buffer)
        .expect("should have written without any errors");
    assert_eq!(buffer, [0x02]);
    buffer.clear();

    Holder { value: 9u8 }
        .write(&mut buffer)
        .expect("should have written without any errors");
    assert_eq!(buffer, [0x09]);
}

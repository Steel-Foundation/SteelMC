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

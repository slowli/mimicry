use mimicry_derive::CallReal;

#[derive(CallReal)]
enum MyMock {
    Some(u32),
    None,
}

fn main() {}

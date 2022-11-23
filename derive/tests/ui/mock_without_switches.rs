use mimicry_derive::CallReal;

#[derive(CallReal)]
struct MyMock {
    not_a_switch: String,
    not_a_switch_either: u32,
}

fn main() {}

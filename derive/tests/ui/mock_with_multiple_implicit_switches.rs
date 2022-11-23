use mimicry_derive::CallReal;

/// Dummy struct to trick `CallReal` derive logic.
struct RealCallSwitch;

#[derive(CallReal)]
struct MyMock {
    switch: RealCallSwitch,
    another_switch: RealCallSwitch,
}

fn main() {}

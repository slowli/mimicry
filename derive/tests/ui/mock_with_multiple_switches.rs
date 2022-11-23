use mimicry_derive::CallReal;

/// Dummy struct to trick `CallReal` derive logic.
struct RealCallSwitch;

#[derive(CallReal)]
struct MyMock {
    #[mock(switch)]
    switch: RealCallSwitch,
    #[mock(switch)]
    another_switch: RealCallSwitch,
}

fn main() {}

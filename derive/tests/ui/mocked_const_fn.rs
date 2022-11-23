use mimicry_derive::mock;

#[mock(using = "MyMock")]
const fn mock_target() -> u32 {
    42
}

fn main() {}

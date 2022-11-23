use mimicry_derive::mock;

#[mock(using = "MyMock")]
pub struct Hello;

#[mock(using = "MyMock")]
pub const ANSWER: u32 = 42;

fn main() {}

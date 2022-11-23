use mimicry_derive::mock;

struct MockTarget;

#[mock(using = "MyMock::test")]
impl MockTarget {
    fn test(&self) -> u32 {
        42
    }
}

fn main() {}

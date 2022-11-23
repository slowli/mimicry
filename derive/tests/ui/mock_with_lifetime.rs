use mimicry_derive::Mock;

#[derive(Mock)]
struct WithLifetime<'a> {
    field: &'a (),
}

fn main() {}

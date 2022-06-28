# Lightweight Mocking / Spying Library for Rust

Mocking in Rust is somewhat hard compared to object-oriented languages. Since there
is no implicit / all-encompassing class hierarchy, [Liskov substitution principle]
does not apply, thus making it generally impossible to replace an object with its mock.
A switch is only possible if the object consumer explicitly opts in via
parametric polymorphism or dynamic dispatch.

What do? Instead of trying to emulate mocking approaches from the object-oriented world,
this crate opts in for another approach, somewhat similar to [remote derive] from `serde`.
Mocking is performed on function / method level, with each function conditionally proxied
to a mock that has access to function args and can do whatever: call the "real" function
(e.g., to spy on responses), maybe with different args and/or after mutating args;
substitute with a mock response, etc. Naturally, mock logic
can be stateful (e.g., determine a response from the predefined list; record responses
for spied functions etc.)

## Usage

Add this to your `Crate.toml`:

```toml
[dev-dependencies]
mimicry = "0.1.0"
```

Example of usage:

```rust
use mimicry::{mock, CallReal, Mock, Mut};

// Tested function
#[mock(using = "SearchMock")]
fn search(haystack: &str, needle: char) -> Option<usize> {
    haystack.chars().position(|ch| ch == needle)
}

// Mock logic
#[derive(Default, Mock)]
#[mock(mut)]
// ^ Indicates that the mock state is wrapped in a wrapper with 
// internal mutability.
struct SearchMock {
    called_times: usize,
}

impl SearchMock {
    // Implementation of mocked function, which the mocked function
    // will delegate to if the mock is set.
    fn search(
        this: &Mut<Self>,
        haystack: &str,
        needle: char,
    ) -> Option<usize> {
        this.borrow().called_times += 1;
        if haystack == "test" {
            Some(42)
        } else {
            let new_needle = if needle == '?' { 'e' } else { needle };
            this.call_real(|| search(haystack, new_needle))
        }
    }
}

// Test code.
let guard = SearchMock::default().set_as_mock();
assert_eq!(search("test", '?'), Some(42));
assert_eq!(search("needle?", '?'), Some(1));
assert_eq!(search("needle?", 'd'), Some(3));
let recovered = guard.into_inner();
assert_eq!(recovered.called_times, 3);
```

## Features

- Can mock functions / methods with a wide variety of signatures, including generic functions
  (with not necessarily `'static` type params), functions returning non-`'static` responses
  and responses with dependent lifetimes, such as in `fn(&str) -> &str`, functions with
  `impl Trait` args etc.
- Can mock methods in `impl` blocks, including trait implementations.
- Single mocking function can mock multiple functions, provided that they have compatible
  signatures.
- Whether mock state is shared across functions / methods, is completely up to the test writer.
  Functions for the same receiver type / in the same `impl` block may have different
  mock states.
- Mocking functions can have wider argument types than required from the signature of
  function(s) being mocked. For example, if the mocking function doesn't use some args,
  they can be just replaced with unconstrained type params.
- No matching via predicates etc. With the chosen approach, it is easier and more transparent
  to just use `match` statements. As a downside, if matching logic needs to be customized
  across tests, it's up to the test writer.

### Downsides

- You still cannot mock types from other crates.
- Even if mocking logic does not use certain args, they need to be properly constructed,
  which, depending on the case, may defy the reasons behind using mocks.

## Alternatives

[`mockall`], [`simulacrum`], [`mocktopus`], [`mockiato`] etc. provide more traditional approach
to mocking based on configuring expectations for called functions / methods.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `mimicry` by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.

[Liskov substitution principle]: https://en.wikipedia.org/wiki/Liskov_substitution_principle
[remote derive]: https://serde.rs/remote-derive.html
[`mockall`]: https://crates.io/crates/mockall
[`simulacrum`]: https://crates.io/crates/simulacrum
[`mocktopus`]: https://crates.io/crates/mocktopus
[`mockiato`]: https://crates.io/crates/mockiato

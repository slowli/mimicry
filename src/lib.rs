//! Mocking / spying library based on lightweight function proxies.
//!
//! Mocking in Rust is somewhat hard compared to object-oriented languages. Since there
//! is no implicit / all-encompassing class hierarchy, [Liskov substitution principle]
//! does not apply, thus making it generally impossible to replace an object with its mock.
//! A switch is only possible if the object consumer explicitly opts in via
//! parametric polymorphism or dynamic dispatch.
//!
//! What do? Instead of trying to emulate mocking approaches from the object-oriented world,
//! this crate opts in for another approach, somewhat similar to [remote derive] from `serde`.
//! Mocking is performed on function / method level, with each function conditionally proxied
//! to a mock that has access to function args and can do whatever: call the "real" function
//! (e.g., to spy on responses), maybe with different args and/or after mutating args;
//! substitute with a mock response, etc. Naturally, mock logic
//! can be stateful (e.g., determine a response from the predefined list; record responses
//! for spied functions etc.)
//!
//! [Liskov substitution principle]: https://en.wikipedia.org/wiki/Liskov_substitution_principle
//! [remote derive]: https://serde.rs/remote-derive.html
//!
//! # Features and limitations
//!
//! - Can mock functions / methods with a wide variety of signatures, including generic functions
//!   (with not necessarily `'static` type params), functions returning non-`'static` responses
//!   and responses with dependent lifetimes, such as in `fn(&str) -> &str`, functions with
//!   `impl Trait` args etc.
//! - Can mock methods in `impl` blocks, including trait implementations.
//! - Single mocking function can mock multiple functions, provided that they have compatible
//!   signatures.
//! - Whether mock state is shared across functions / methods, is completely up to the test writer.
//!   Functions for the same receiver type / in the same `impl` block may have different
//!   mock states.
//! - Mocking functions can have wider argument types than required from the signature of
//!   function(s) being mocked. For example, if the mocking function doesn't use some args,
//!   they can be just replaced with unconstrained type params.
//! - No matching via predicates etc. With the chosen approach, it is easier and more transparent
//!   to just use `match` statements. As a downside, if matching logic needs to be customized
//!   across tests, it's up to the test writer.
//! - Cannot mock types from other crates.
//! - Even if mocking logic does not use certain args, they need to be properly constructed,
//!   which, depending on the case, may defy the reasons behind using mocks.
//!
//! # Examples
//!
//! ## Basics
//!
//! ```
//! use mimicry::{mock, Context, Mock, SetMock};
//!
//! // Mock target: a standalone function.
//! #[mock(using = "SearchMock")]
//! // ^ In real uses, this attr would be wrapped in a condition
//! // such as `#[cfg_attr(test, _)]`.
//! fn search(haystack: &str, needle: char) -> Option<usize> {
//!     haystack.chars().position(|ch| ch == needle)
//! }
//!
//! // Mock state. In this case, we use it to record responses.
//! #[derive(Default, Mock)]
//! struct SearchMock {
//!     called_times: usize,
//! }
//!
//! impl SearchMock {
//!     // Mock implementation: an inherent method of the mock state
//!     // specified in the `#[mock()]` macro on the mocked function.
//!     // The mock impl receives same args as the mocked function
//!     // with the additional context parameter that allows
//!     // accessing the mock state and controlling mock / real function switches.
//!     fn search(
//!         mut ctx: Context<'_, Self>,
//!         haystack: &str,
//!         needle: char,
//!     ) -> Option<usize> {
//!         ctx.state().called_times += 1;
//!         match haystack {
//!             "test" => Some(42),
//!             short if short.len() <= 2 => None,
//!             _ => {
//!                 let new_needle = if needle == '?' { 'e' } else { needle };
//!                 ctx.fallback(|| search(haystack, new_needle))
//!             }
//!         }
//!     }
//! }
//!
//! // Test code.
//! let guard = SearchMock::instance().set_default();
//! assert_eq!(search("test", '?'), Some(42));
//! assert_eq!(search("?!", '?'), None);
//! assert_eq!(search("needle?", '?'), Some(1));
//! assert_eq!(search("needle?", 'd'), Some(3));
//! let recovered = guard.into_inner();
//! assert_eq!(recovered.called_times, 4);
//! ```
//!
//! ## On impl blocks
//!
//! The `mock` attribute can be placed on impl blocks (including trait implementations)
//! to apply a mock to all methods in the block:
//!
//! ```
//! # use mimicry::{mock, Context, Mock};
//! struct Tested(String);
//!
//! #[mock(using = "TestMock")]
//! impl Tested {
//!     fn len(&self) -> usize { self.0.len() }
//!
//!     fn push(&mut self, s: impl AsRef<str>) -> &mut Self {
//!         self.0.push_str(s.as_ref());
//!         self
//!     }
//! }
//!
//! #[mock(using = "TestMock", rename = "impl_{}")]
//! impl AsRef<str> for Tested {
//!     fn as_ref(&self) -> &str {
//!         &self.0
//!     }
//! }
//!
//! #[derive(Mock)]
//! struct TestMock { /* ... */ }
//! impl TestMock {
//!     fn len(ctx: Context<'_, Self>, recv: &Tested) -> usize {
//!         // ...
//!         # 0
//!     }
//!
//!     fn push<'s>(
//!         ctx: Context<'_, Self>,
//!         recv: &'s mut Tested,
//!         s: impl AsRef<str>,
//!     ) -> &'s mut Tested {
//!         // ...
//!         # recv
//!     }
//!
//!     fn impl_as_ref<'s>(ctx: Context<'_, Self>, recv: &'s Tested) -> &'s str {
//!         // ...
//!         # ""
//!     }
//! }
//! ```
//!
//! ## What can('t) be mocked?
//!
//! ```
//! # use mimicry::{mock, Context, Mock, SetMock};
//! struct Test;
//! impl Test {
//!     #[mock(using = "CountingMock::count")]
//!     fn do_something(&self) {}
//!
//!     #[mock(using = "CountingMock::count")]
//!     fn lifetimes(&self) -> &str {
//!         "what?"
//!     }
//!
//!     #[mock(using = "CountingMock::count")]
//!     fn generics<T: ToOwned>(value: &T) -> Vec<T::Owned> {
//!         (0..5).map(|_| value.to_owned()).collect()
//!     }
//!
//!     #[mock(using = "CountingMock::count")]
//!     fn impl_methods(value: &impl AsRef<str>) -> &str {
//!         value.as_ref()
//!     }
//! }
//!
//! impl Iterator for Test {
//!     type Item = u8;
//!
//!     #[mock(using = "CountingMock::count")]
//!     fn next(&mut self) -> Option<Self::Item> {
//!         Some(42)
//!     }
//! }
//!
//! #[derive(Default, Mock)]
//! struct CountingMock(usize);
//! impl CountingMock {
//!     // All functions above can be mocked with a single impl!
//!     // This is quite extreme, obviously; in realistic scenarios,
//!     // you probably wouldn't be able to unite mocks of functions
//!     // with significantly differing return types.
//!     fn count<T, R: Default>(mut ctx: Context<'_, Self>, _: T) -> R {
//!         ctx.state().0 += 1;
//!         R::default()
//!     }
//! }
//!
//! let guard = CountingMock::instance().set_default();
//! Test.do_something();
//! assert_eq!(Test.lifetimes(), "");
//! assert_eq!(Test.next(), None);
//! let count = guard.into_inner().0;
//! assert_eq!(count, 3);
//! ```

// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

use once_cell::sync::OnceCell;

#[cfg(feature = "shared")]
mod shared;
mod tls;

#[cfg(feature = "shared")]
pub use crate::shared::{Shared, SharedGuard, SharedRef};
pub use crate::tls::{ThreadLocal, ThreadLocalGuard, ThreadLocalRef};
pub use mimicry_derive::{mock, Mock};

use core::{
    cell::{Cell, RefCell, RefMut},
    ops,
};

/// Interface to get mock state.
#[doc(hidden)] // only used in macros
pub trait GetMock<'a, T> {
    /// Reference to the shared mock state. This is required as a separate entity to only
    /// call the mock impls when appropriate (non-`Copy` / non-autoref'd args
    /// are consumed by the call, so we must be extra careful to only call the mock impl
    /// when we know it's there).
    type Ref: CallMock<T> + 'a;

    /// Returns a reference to the shared mock state, or `None` if the mock is not set.
    fn get(&'a self) -> Option<Self::Ref>;
}

/// Interface to set up mock state.
pub trait SetMock<'a, T> {
    /// Exclusive guard for the mock. See [`Self::set()`] for more details.
    type Guard: 'a;

    /// Sets the mock state.
    ///
    /// # Return value
    ///
    /// Returns an exclusive guard to the shared state. Can be used to check / adjust
    /// the mock state during the test. Dropping the guard also unsets the mock state,
    /// so that targeted functions are no longer mocked.
    ///
    /// In case of [shared mocks], guards also provided synchronization across concurrently
    /// executing tests: until a guard is dropped, other threads attempting
    /// to call [`Self::set()`] will block. Unfortunately, this is not always sufficient
    /// to have good results; see [`Shared`](crate::Shared) docs for discussion.
    ///
    /// [shared mocks]: crate::Shared
    fn set(&'a self, state: T) -> Self::Guard;

    /// Sets the default mock state.
    fn set_default(&'a self) -> Self::Guard
    where
        T: Default,
    {
        self.set(T::default())
    }
}

/// Interface to lock mock state changes without [setting](SetMock) the state.
pub trait LockMock<'a, T>: SetMock<'a, T> {
    /// Exclusive guard for the mock that does not contain the state.
    type EmptyGuard: 'a;

    /// Locks access to the mock state without setting the state. This is useful
    /// for [shared mocks] to ensure that tests not using mocks do not observe mocks
    /// set by other tests.
    ///
    /// [shared mocks]: crate::Shared
    fn lock(&'a self) -> Self::EmptyGuard;
}

#[doc(hidden)] // only used in macros
pub trait CallMock<T> {
    fn call_mock<R>(self, switch: &FallbackSwitch, action: impl FnOnce(Context<'_, T>) -> R) -> R;
}

/// Wrapper that allows creating `static`s with [`SetMock`] implementations.
#[derive(Debug)]
pub struct Static<T> {
    cell: OnceCell<T>,
}

impl<T> Static<T> {
    /// Creates a new instance.
    pub const fn new() -> Self {
        Self {
            cell: OnceCell::new(),
        }
    }
}

impl<'a, T, S> GetMock<'a, T> for Static<S>
where
    S: GetMock<'a, T> + Default,
{
    type Ref = S::Ref;

    fn get(&'a self) -> Option<Self::Ref> {
        let cell = self.cell.get_or_init(S::default);
        cell.get()
    }
}

impl<'a, T, S> SetMock<'a, T> for Static<S>
where
    S: SetMock<'a, T> + Default,
{
    type Guard = S::Guard;

    fn set(&'a self, state: T) -> Self::Guard {
        let cell = self.cell.get_or_init(S::default);
        cell.set(state)
    }
}

impl<'a, T, S> LockMock<'a, T> for Static<S>
where
    S: LockMock<'a, T> + Default,
{
    type EmptyGuard = S::EmptyGuard;

    fn lock(&'a self) -> Self::EmptyGuard {
        let cell = self.cell.get_or_init(S::default);
        cell.lock()
    }
}

/// State of a mock.
pub trait Mock: Sized {
    /// Wrapper around this state allowing to share it across test code and the main program.
    type Shared: for<'a> GetMock<'a, Self> + for<'a> SetMock<'a, Self> + 'static + Send + Sync;

    /// Returns the shared wrapper around this state.
    fn instance() -> &'static Static<Self::Shared>;
}

/// Context providing access to the mock state and fallback switches for mocked functions.
#[derive(Debug)]
pub struct Context<'a, T> {
    state: &'a RefCell<Option<T>>, // `Option` is always `Some(_)`
    fallback_switch: &'a FallbackSwitch,
}

impl<'a, T> Context<'a, T> {
    fn new(state: &'a RefCell<Option<T>>, fallback_switch: &'a FallbackSwitch) -> Self {
        Self {
            state,
            fallback_switch,
        }
    }

    /// Re-borrows this context for a shorter duration.
    pub fn borrow(&mut self) -> Context<'_, T> {
        Context {
            state: self.state,
            fallback_switch: self.fallback_switch,
        }
    }

    /// Returns an exclusive reference to the mock state.
    ///
    /// Beware that while the reference is alive, further calls to functions in the same mock
    /// (including indirect ones, e.g., performed from the tested program code)
    /// will not be able to retrieve the state; this will result
    /// in a panic. To deal with this, you can create short lived state refs a la
    /// `mock.state().do_something()`, or enclose the reference into an additional scope.
    ///
    /// # Panics
    ///
    /// Panics if a reference to the same mock state is alive, as described above.
    pub fn state(&mut self) -> impl ops::DerefMut<Target = T> + '_ {
        RefMut::map(self.state.borrow_mut(), |state| state.as_mut().unwrap())
    }

    /// Runs the provided closure with all calls to the mocked function / method being
    /// directed to "real" implementation.
    pub fn fallback<R>(&mut self, action: impl FnOnce() -> R) -> R {
        self.fallback_switch.0.set(FallbackState::Fallback);
        let _guard = FallbackGuard {
            switch: self.fallback_switch,
        };
        action()
    }

    /// Runs the provided closure with the *first* call to the mocked function / method being
    /// directed to "real" implementation. Further calls will be directed to the mock.
    pub fn fallback_once<R>(&mut self, action: impl FnOnce() -> R) -> R {
        self.fallback_switch.0.set(FallbackState::FallbackOnce);
        let _guard = FallbackGuard {
            switch: self.fallback_switch,
        };
        action()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FallbackState {
    Inactive,
    Fallback,
    FallbackOnce,
}

impl Default for FallbackState {
    fn default() -> Self {
        Self::Inactive
    }
}

/// Switch to (de)activate fallback implementations of mocked functions.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct FallbackSwitch(Cell<FallbackState>);

impl FallbackSwitch {
    pub fn is_active(&self) -> bool {
        let state = self.0.get();
        if state == FallbackState::FallbackOnce {
            self.0.set(FallbackState::Inactive);
        }
        state != FallbackState::Inactive
    }
}

#[derive(Debug)]
struct FallbackGuard<'a> {
    switch: &'a FallbackSwitch,
}

impl Drop for FallbackGuard<'_> {
    fn drop(&mut self) {
        self.switch.0.set(FallbackState::Inactive);
    }
}

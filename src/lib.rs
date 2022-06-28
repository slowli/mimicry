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
//!
//! ## Downsides
//!
//! - You still cannot mock types from other crates.
//! - Even if mocking logic does not use certain args, they need to be properly constructed,
//!   which, depending on the case, may defy the reasons behind using mocks.
//!
//! # Crate features
//!
//! ## `shared`
//!
//! *(Off by default)*
//!
//! Enables [mocks](Shared) that can be used across multiple threads.
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

// Documentation settings.
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(html_root_url = "https://docs.rs/mimicry/0.1.0")]
// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

use once_cell::sync::OnceCell;

use core::{
    cell::{Cell, RefCell},
    fmt, ops,
};

#[cfg(feature = "shared")]
mod shared;
mod tls;

#[cfg(feature = "shared")]
pub use crate::shared::{Shared, SharedGuard};
pub use crate::tls::{ThreadLocal, ThreadLocalGuard};
pub use mimicry_derive::{mock, Mock};

/// Interface to get mock state.
#[doc(hidden)] // only used in macros
pub trait GetMock<'a, T> {
    /// Reference to the shared mock state. This is required as a separate entity to only
    /// call the mock impls when appropriate (non-`Copy` / non-autoref'd args
    /// are consumed by the call, so we must be extra careful to only call the mock impl
    /// when we know it's there).
    type Ref: ops::Deref<Target = T> + 'a;

    /// Returns a reference to the shared mock state, or `None` if the mock is not set.
    fn get(&'a self) -> Option<Self::Ref>;
}

/// Interface to set up mock state.
pub trait SetMock<'a, T> {
    /// Exclusive guard for the mock. See [`Self::set()`] for more details.
    type Guard: 'a + Guard<T>;

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
}

#[doc(hidden)]
pub trait Guard<T> {
    fn with<R>(&mut self, action: impl FnOnce(&mut T) -> R) -> R;
    fn into_inner(self) -> T;
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

/// Wrapper that allows proxying exclusive accesses to the wrapped object. `Wrap<T>`
/// is similar to `Into<T> + BorrowMut<T>`, but without the necessity to implement `Borrow<T>`
/// (which would be unsound for the desired use cases), or deal with impossibility to
/// blanket-implement `Into<T>`.
pub trait Wrap<T>: From<T> {
    /// Returns the wrapped value.
    fn into_inner(self) -> T;
    /// Returns an exclusive reference to the wrapped value.
    fn as_mut(&mut self) -> &mut T;
}

impl<T> Wrap<T> for T {
    fn into_inner(self) -> T {
        self
    }

    fn as_mut(&mut self) -> &mut T {
        self
    }
}

/// State of a mock.
pub trait Mock: Sized {
    /// FIXME
    type Base: Wrap<Self> + CheckDelegate;

    /// Wrapper around [`Self::Base`] allowing to share it across test code and the main program.
    #[doc(hidden)]
    type Shared: GetMock<'static, Self::Base>
        + SetMock<'static, Self::Base>
        + 'static
        + Default
        + Send
        + Sync;

    /// Returns the shared wrapper around this state.
    #[doc(hidden)]
    fn instance() -> &'static Static<Self::Shared>;

    /// FIXME
    fn set(state: Self) -> MockGuard<Self> {
        let cell = Self::instance().cell.get_or_init(<Self::Shared>::default);
        MockGuard {
            inner: cell.set(state.into()),
        }
    }

    /// FIXME
    fn set_default() -> MockGuard<Self>
    where
        Self: Default,
    {
        Self::set(Self::default())
    }

    /// FIXME
    fn lock() -> EmptyGuard<Self>
    where
        Self::Shared: LockMock<'static, Self::Base>,
    {
        let cell = Self::instance().cell.get_or_init(<Self::Shared>::default);
        EmptyGuard {
            _inner: cell.lock(),
        }
    }
}

/// FIXME
pub struct MockGuard<T: Mock> {
    inner: <T::Shared as SetMock<'static, T::Base>>::Guard,
}

impl<T: Mock> fmt::Debug for MockGuard<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("MockGuard").finish_non_exhaustive()
    }
}

impl<T: Mock> MockGuard<T> {
    /// Performs an action on the mock state without releasing the guard. This can be used
    /// to adjust the mock state, check or take some parts of it (such as responses).
    pub fn with<R>(&mut self, action: impl FnOnce(&mut T) -> R) -> R {
        self.inner.with(|wrapped| action(wrapped.as_mut()))
    }

    /// Returns the enclosed mock state and releases the exclusive lock.
    pub fn into_inner(self) -> T {
        Guard::into_inner(self.inner).into_inner()
    }
}

/// FIXME
pub struct EmptyGuard<T: Mock>
where
    T::Shared: LockMock<'static, T::Base>,
{
    _inner: <T::Shared as LockMock<'static, T::Base>>::EmptyGuard,
}

impl<T: Mock> fmt::Debug for EmptyGuard<T>
where
    T::Shared: LockMock<'static, T::Base>,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("EmptyGuard").finish_non_exhaustive()
    }
}

/// FIXME
#[derive(Debug, Default)]
pub struct Mut<T> {
    inner: RefCell<T>,
    switch: DelegateSwitch,
}

impl<T> Mut<T> {
    /// FIXME
    pub fn borrow(&self) -> impl ops::DerefMut<Target = T> + '_ {
        self.inner.borrow_mut()
    }

    /// FIXME
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }
}

impl<T> From<T> for Mut<T> {
    fn from(inner: T) -> Self {
        Self {
            inner: RefCell::new(inner),
            switch: DelegateSwitch::default(),
        }
    }
}

impl<T> Wrap<T> for Mut<T> {
    fn into_inner(self) -> T {
        self.inner.into_inner()
    }

    fn as_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }
}

impl<T> Delegate for Mut<T> {
    fn delegate_switch(&self) -> &DelegateSwitch {
        &self.switch
    }
}

/// FIXME
pub trait CheckDelegate {
    /// FIXME
    fn should_delegate(&self) -> bool {
        false
    }
}

/// FIXME
pub trait Delegate {
    /// FIXME
    fn delegate_switch(&self) -> &DelegateSwitch;

    /// FIXME
    fn call_real<R>(&self, action: impl FnOnce() -> R) -> R {
        let switch = <Self as Delegate>::delegate_switch(self);
        switch.0.set(DelegateMode::RealImpl);
        let _guard = DelegateGuard { switch };
        action()
    }

    /// FIXME
    fn call_real_once<R>(&self, action: impl FnOnce() -> R) -> R {
        let switch = <Self as Delegate>::delegate_switch(self);
        switch.0.set(DelegateMode::RealImplOnce);
        let _guard = DelegateGuard { switch };
        action()
    }
}

impl<T: Delegate> CheckDelegate for T {
    fn should_delegate(&self) -> bool {
        self.delegate_switch().should_delegate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DelegateMode {
    Inactive,
    RealImpl,
    RealImplOnce,
}

impl Default for DelegateMode {
    fn default() -> Self {
        Self::Inactive
    }
}

/// Switch to (de)activate fallback implementations of mocked functions.
#[derive(Debug, Default)]
pub struct DelegateSwitch(Cell<DelegateMode>);

#[doc(hidden)]
impl DelegateSwitch {
    pub fn should_delegate(&self) -> bool {
        let mode = self.0.get();
        if mode == DelegateMode::RealImplOnce {
            self.0.set(DelegateMode::Inactive);
        }
        mode != DelegateMode::Inactive
    }
}

#[derive(Debug)]
struct DelegateGuard<'a> {
    switch: &'a DelegateSwitch,
}

impl Drop for DelegateGuard<'_> {
    fn drop(&mut self) {
        self.switch.0.set(DelegateMode::Inactive);
    }
}

#[cfg(doctest)]
doc_comment::doctest!("../README.md");

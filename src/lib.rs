//! Mocking / spying library based on lightweight function proxies.
//!
//! Mocking in Rust is somewhat hard compared to object-oriented languages. Since there
//! is no implicit / all-encompassing class hierarchy, [Liskov substitution principle]
//! does not apply, making it generally impossible to replace an object with its mock.
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
//! # Overview
//!
//! 1. Define the state to hold data necessary for mocking / spying and derive
//!   [`Mock`] for it. Requirements to the state are quite lax; it should be
//!  `'static` and `Send`.
//! 2. Place [`mock`] attrs referencing the state on the relevant functions, methods
//!   and/or impl blocks.
//! 3. Define mock logic as inherent methods of the mock state type. Such methods will be called
//!   with the same args as the original functions + additional first arg for the mock state
//!   reference. In the simplest case,
//!   each mocked function / method gets its own method with the same name as the original,
//!   but this can be customized.
//! 4. If the state needs to be mutated in mock logic, add a `#[mock(mut)]` attr on the state.
//!   In this case, the mock method will receive `&`[`Mut`]`<Self>` wrapper as the first arg
//!   instead of `&self`.
//! 5. If the mock logic needs to be shared across threads, add a `#[mock(shared)]` attr
//!   on the state. (By default, mocks are thread-local.)
//! 6. Set the mock state in tests using [`Mock::set_as_mock()`]. Inspect the state during tests
//!   using [`MockGuard::with()`] and after tests using [`MockGuard::into_inner()`].
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
//!
//! ## Downsides
//!
//! - You still cannot mock types from other crates.
//! - Even if mocking logic does not use certain args, they need to be properly constructed,
//!   which, depending on the case, may defy the reasons behind using mocks.
//! - Very limited built-in matching / verifying (see [`Answers`]). With the chosen approach,
//!   it is frequently easier and more transparent to just use `match` statements.
//!   As a downside, if matching logic needs to be customized across tests, it's (mostly)
//!   up to the test writer.
//!
//! # Crate features
//!
//! ## `shared`
//!
//! *(Off by default)*
//!
//! Enables mocks that [can be used](Shared) across multiple threads.
//!
//! # Examples
//!
//! ## Basics
//!
//! ```
//! use mimicry::{mock, CallReal, RealCallSwitch, Mock};
//! # use std::cell::Cell;
//!
//! // Mock target: a standalone function.
//! #[cfg_attr(test, mock(using = "SearchMock"))]
//! # fn eat_attr() {}
//! # #[mock(using = "SearchMock")]
//! fn search(haystack: &str, needle: char) -> Option<usize> {
//!     haystack.chars().position(|ch| ch == needle)
//! }
//!
//! // Mock state. In this case, we use it to record responses.
//! #[derive(Default, Mock, CallReal)]
//! struct SearchMock {
//!     called_times: Cell<usize>,
//!     switch: RealCallSwitch,
//!     // ^ Stores the real / mocked function switch, thus allowing
//!     // to call `Delegate` trait methods.
//! }
//!
//! impl SearchMock {
//!     // Mock implementation: an inherent method of the mock state
//!     // specified in the `#[mock()]` macro on the mocked function.
//!     // The mock impl receives same args as the mocked function
//!     // with the additional context parameter that allows
//!     // accessing the mock state and controlling mock / real function switches.
//!     fn search(
//!         &self,
//!         haystack: &str,
//!         needle: char,
//!     ) -> Option<usize> {
//!         self.called_times.set(self.called_times.get() + 1);
//!         match haystack {
//!             "test" => Some(42),
//!             short if short.len() <= 2 => None,
//!             _ => {
//!                 let new_needle = if needle == '?' { 'e' } else { needle };
//!                 self.call_real(|| search(haystack, new_needle))
//!             }
//!         }
//!     }
//! }
//!
//! // Test code.
//! let guard = SearchMock::default().set_as_mock();
//! assert_eq!(search("test", '?'), Some(42));
//! assert_eq!(search("?!", '?'), None);
//! assert_eq!(search("needle?", '?'), Some(1));
//! assert_eq!(search("needle?", 'd'), Some(3));
//! let recovered = guard.into_inner();
//! assert_eq!(recovered.called_times.into_inner(), 4);
//! ```
//!
//! Mock functions only get a shared reference to the mock state; this is because
//! the same state can be accessed from multiple places during recursive calls.
//! To easily mutate the state during tests, consider using the [`Mut`]
//! wrapper.
//!
//! ## On impl blocks
//!
//! The `mock` attribute can be placed on impl blocks (including trait implementations)
//! to apply a mock to all methods in the block:
//!
//! ```
//! # use mimicry::{mock, CheckRealCall, Mock};
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
//! // Since we don't use partial mocking / spying, we indicate
//! // this with an empty `CheckRealCall` impl.
//! impl CheckRealCall for TestMock {}
//!
//! impl TestMock {
//!     fn len(&self, recv: &Tested) -> usize {
//!         // ...
//!         # 0
//!     }
//!
//!     fn push<'s>(
//!         &self,
//!         recv: &'s mut Tested,
//!         s: impl AsRef<str>,
//!     ) -> &'s mut Tested {
//!         // ...
//!         # recv
//!     }
//!
//!     fn impl_as_ref<'s>(&self, recv: &'s Tested) -> &'s str {
//!         // ...
//!         # ""
//!     }
//! }
//! ```
//!
//! ## What can('t) be mocked?
//!
//! ```
//! # use mimicry::{mock, CheckRealCall, Mock};
//! # use std::sync::atomic::{AtomicU32, Ordering};
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
//! struct CountingMock(AtomicU32);
//!
//! impl CheckRealCall for CountingMock {}
//!
//! impl CountingMock {
//!     // All functions above can be mocked with a single impl!
//!     // This is quite extreme, obviously; in realistic scenarios,
//!     // you probably wouldn't be able to unite mocks of functions
//!     // with significantly differing return types.
//!     fn count<T, R: Default>(&self, _: T) -> R {
//!         self.0.fetch_add(1, Ordering::Relaxed);
//!         R::default()
//!     }
//! }
//!
//! let guard = CountingMock::default().set_as_mock();
//! Test.do_something();
//! assert_eq!(Test.lifetimes(), "");
//! assert_eq!(Test.next(), None);
//! let count = guard.into_inner().0;
//! assert_eq!(count.into_inner(), 3);
//! ```

// Documentation settings.
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(html_root_url = "https://docs.rs/mimicry/0.1.0")]
// Linter settings.
#![warn(missing_debug_implementations, missing_docs, bare_trait_objects)]
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

use once_cell::sync::OnceCell;

use core::{cell::RefCell, fmt, ops};

mod answers;
#[cfg(feature = "shared")]
mod shared;
mod tls;
mod traits;

#[cfg(feature = "shared")]
pub use crate::shared::Shared;
pub use crate::{
    answers::{Answers, AnswersGuard, AnswersSender},
    tls::ThreadLocal,
    traits::{CallReal, CheckRealCall, GetMock, RealCallSwitch},
};
pub use mimicry_derive::{mock, CallReal, Mock};

use crate::traits::{Guard, LockMock, SetMock, Wrap};

/// Wrapper that allows creating `static`s with mock state.
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

/// State of a mock.
///
/// This trait should be implemented via the corresponding derive macro; parts of it are
/// non-documented and subject to change.
pub trait Mock: Sized {
    #[doc(hidden)]
    type Base: Wrap<Self> + CheckRealCall;

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

    /// Sets the mock state and returns an exclusive guard to the shared state.
    #[must_use = "mock is only set until the returned `MockGuard` is dropped"]
    fn set_as_mock(self) -> MockGuard<Self> {
        let cell = Self::instance().cell.get_or_init(<Self::Shared>::default);
        MockGuard {
            inner: cell.set(self.into()),
        }
    }

    /// Locks write access to the mock state without setting the state. This is useful
    /// for [shared mocks] to ensure that tests not using mocks do not observe mocks
    /// set by other tests.
    ///
    /// [shared mocks]: crate::Shared
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

/// Exclusive guard to set the mock state.
///
/// A guard can be used to check / adjust the mock state during the test.
/// Dropping the guard also unsets the mock state, so that targeted functions are no longer mocked.
///
/// In case of [shared mocks], guards also provided synchronization across concurrently
/// executing tests: until a guard is dropped, other threads attempting
/// to call [`Mock::set_as_mock()`] will block. Unfortunately, this is not always sufficient
/// to have good results; see [`Shared`](crate::Shared) docs for discussion.
///
/// [shared mocks]: crate::Shared
///
/// # Examples
///
/// ```
/// # use mimicry::{mock, CheckRealCall, Mock, MockGuard};
/// #[mock(using = "ValueMock")]
/// fn answer() -> usize { 42 }
///
/// #[derive(Default, Mock)]
/// struct ValueMock(usize);
///
/// impl CheckRealCall for ValueMock {}
///
/// impl ValueMock {
///     fn answer(&self) -> usize {
///         self.0
///     }
/// }
///
/// assert_eq!(answer(), 42);
/// let mut guard: MockGuard<_> = ValueMock::default().set_as_mock();
/// assert_eq!(answer(), 0);
/// guard.with(|mock| { mock.0 = 23; });
/// // ^ updates mock state without releasing the guard
/// assert_eq!(answer(), 23);
/// ```
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
    /// to adjust the mock state, check or take some parts of it (such as collected args
    /// or responses).
    pub fn with<R>(&mut self, action: impl FnOnce(&mut T) -> R) -> R {
        self.inner.with(|wrapped| action(wrapped.as_mut()))
    }

    /// Returns the enclosed mock state and releases the exclusive lock.
    pub fn into_inner(self) -> T {
        Guard::into_inner(self.inner).into_inner()
    }
}

/// Exclusive guard to set the mock state without an attached state.
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

/// Reference to a mock state used when mocking async functions / methods.
///
/// # Examples
///
/// FIXME
#[derive(Debug)]
pub struct MockRef<T: Mock> {
    instance: &'static Static<T::Shared>,
}

impl<T: Mock> Clone for MockRef<T> {
    fn clone(&self) -> Self {
        Self {
            instance: self.instance,
        }
    }
}

impl<T: Mock> Copy for MockRef<T> {}

impl<T: Mock> MockRef<T> {
    #[doc(hidden)] // used by the `mock` macro
    pub fn new(instance: &'static Static<T::Shared>) -> Self {
        Self { instance }
    }
}

impl<T: Mock<Base = T>> MockRef<T> {
    /// Accesses the underlying mock state.
    ///
    /// # Panics
    ///
    /// Panics if the mock state has gone missing. This is a sign that test code is ill-constructed
    /// (e.g., the mock is removed before all mocked calls are made).
    pub fn with<R>(&self, action: impl FnOnce(&T) -> R) -> R {
        if let Some(mock_ref) = GetMock::get(self.instance) {
            action(&*mock_ref)
        } else {
            panic!("mock state is gone");
        }
    }
}

impl<T: Mock<Base = Mut<T>>> MockRef<T> {
    /// Accesses the underlying [`Mut`]able mock state.
    ///
    /// # Panics
    ///
    /// Panics if the mock state has gone missing. This is a sign that test code is ill-constructed
    /// (e.g., the mock is removed before all mocked calls are made).
    pub fn with_mut<R>(&self, action: impl FnOnce(&mut T) -> R) -> R {
        if let Some(mock_ref) = GetMock::get(self.instance) {
            let base: &Mut<T> = &mock_ref;
            action(&mut base.borrow())
        } else {
            panic!("mock state is gone");
        }
    }
}

/// A lightweight wrapper around the state (essentially, a [`RefCell`]) allowing to easily
/// mutate it in mock code.
///
/// Besides access to the state, `Mut` implements [`CallReal`], thus allowing
/// partial mocks / spies.
///
/// # Examples
///
/// ```
/// # use mimicry::{mock, Mock, MockGuard, Mut};
/// #[mock(using = "CounterMock")]
/// fn answer() -> usize { 42 }
///
/// #[derive(Default, Mock)]
/// #[mock(mut)] // indicates to use `Mut`
/// struct CounterMock(usize);
///
/// impl CounterMock {
///     fn answer(this: &Mut<Self>) -> usize {
///         // Note a custom "receiver" instead of `&self`
///         this.borrow().0 += 1;
///         this.borrow().0
///     }
/// }
///
/// let guard = CounterMock::default().set_as_mock();
/// assert_eq!(answer(), 1);
/// assert_eq!(answer(), 2);
/// assert_eq!(answer(), 3);
/// assert_eq!(guard.into_inner().0, 3);
/// ```
#[derive(Debug, Default)]
pub struct Mut<T> {
    inner: RefCell<T>,
    switch: RealCallSwitch,
}

impl<T> Mut<T> {
    /// Returns an exclusive reference to the underlying mock.
    ///
    /// Beware that while the reference is alive, further calls to functions in the same mock
    /// (including indirect ones, e.g., performed from the tested program code)
    /// will not be able to retrieve the state via this method; this will result
    /// in a panic. To deal with this, you can create short lived state refs a la
    /// `this.borrow().do_something()`, or enclose the reference into an additional scope.
    ///
    /// # Panics
    ///
    /// Panics if a reference to the same mock state is alive, as described above.
    pub fn borrow(&self) -> impl ops::DerefMut<Target = T> + '_ {
        self.inner.borrow_mut()
    }
}

impl<T> From<T> for Mut<T> {
    fn from(inner: T) -> Self {
        Self {
            inner: RefCell::new(inner),
            switch: RealCallSwitch::default(),
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

impl<T> CallReal for Mut<T> {
    fn real_switch(&self) -> &RealCallSwitch {
        &self.switch
    }
}

#[cfg(doctest)]
doc_comment::doctest!("../README.md");

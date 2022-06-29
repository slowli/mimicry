//! Lower-level traits used to generalize the concept of mock state shared between tests
//! and the tested code.

use core::{cell::Cell, ops};

/// Interface to get mock state.
#[doc(hidden)] // only used by generated code
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
    type Guard: 'a + Guard<T>;

    fn set(&'a self, state: T) -> Self::Guard;
}

/// Guard for setting mock state from the test code.
pub trait Guard<T> {
    fn with<R>(&mut self, action: impl FnOnce(&mut T) -> R) -> R;

    fn into_inner(self) -> T;
}

/// Interface to lock mock state changes without [setting](SetMock) the state.
#[doc(hidden)]
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

/// Checks whether it is necessary to delegate to real impl instead of the mock.
pub trait CheckRealCall {
    /// Performs the check.
    ///
    /// The default implementation always returns `false` (i.e., always use the mock).
    fn should_call_real(&self) -> bool {
        false
    }
}

/// Controls delegation to real impls. The provided `call_*` methods in this trait can be used
/// for partial mocking and spying.
pub trait CallReal {
    /// Returns a reference to the call switch.
    fn real_switch(&self) -> &RealCallSwitch;

    /// Runs the provided closure with all calls to the mocked function / method being
    /// directed to "real" implementation.
    fn call_real<R>(&self, action: impl FnOnce() -> R) -> R {
        let switch = <Self as CallReal>::real_switch(self);
        switch.0.set(RealCallMode::Always);
        let _guard = RealCallGuard { switch };
        action()
    }

    /// Runs the provided closure with the *first* call to the mocked function / method being
    /// directed to "real" implementation. Further calls will be directed to the mock.
    fn call_real_once<R>(&self, action: impl FnOnce() -> R) -> R {
        let switch = <Self as CallReal>::real_switch(self);
        switch.0.set(RealCallMode::Once);
        let _guard = RealCallGuard { switch };
        action()
    }
}

impl<T: CallReal> CheckRealCall for T {
    fn should_call_real(&self) -> bool {
        self.real_switch().should_delegate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RealCallMode {
    Inactive,
    Always,
    Once,
}

impl Default for RealCallMode {
    fn default() -> Self {
        Self::Inactive
    }
}

/// Switch between real and mocked implementations.
///
/// A field of this type should be present on a struct for `#[derive(CallReal)]` to work.
///
/// # Examples
///
/// ```
/// # use mimicry::{CallReal, Mock, RealCallSwitch};
/// #[derive(Mock, CallReal)]
/// struct MockState {
///     // other fields...
///     _switch: RealCallSwitch,
/// }
///
/// // You can now use `CallReal` methods in mock logic:
/// impl MockState {
///     fn mock_something(&self, arg: &str) {
///         self.call_real(|| { /* ... */ });
///     }
/// }
/// ```
///
/// The derive logic is nothing magical; it can be easily replicated manually if necessary:
///
/// ```
/// # use mimicry::{CallReal, RealCallSwitch};
/// struct MockState {
///     // other fields...
///     switch: RealCallSwitch,
/// }
///
/// impl CallReal for MockState {
///     fn real_switch(&self) -> &RealCallSwitch {
///         &self.switch
///     }
/// }
/// ```
#[derive(Debug, Default)]
pub struct RealCallSwitch(Cell<RealCallMode>);

impl RealCallSwitch {
    fn should_delegate(&self) -> bool {
        let mode = self.0.get();
        if mode == RealCallMode::Once {
            self.0.set(RealCallMode::Inactive);
        }
        mode != RealCallMode::Inactive
    }
}

#[derive(Debug)]
struct RealCallGuard<'a> {
    switch: &'a RealCallSwitch,
}

impl Drop for RealCallGuard<'_> {
    fn drop(&mut self) {
        self.switch.0.set(RealCallMode::Inactive);
    }
}

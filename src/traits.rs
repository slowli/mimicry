//! Lower-level traits used to generalize the concept of mock state shared between tests
//! and the tested code.

use core::{cell::Cell, future::Future, ops};

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
///
/// This trait can be derived using the corresponding macro; it's not intended
/// for manual implementation. The trait is also implemented for the [`Mut`](crate::Mut)
/// and [`MockRef`](crate::MockRef) wrappers.
///
/// # Call guard checks
///
/// [`RealCallGuard`]s returned by [`Self::call_real()`] and [`Self::call_real_once()`]
/// must not overlap in terms of their lifetime; otherwise, confusion would arise as to
/// which calls exactly should be delegated to real implementations. This is checked
/// in runtime when creating a guard.
///
/// ```should_panic
/// # use mimicry::{mock, CallReal, Mock, RealCallSwitch};
/// #[mock(using = "MyMock")]
/// fn answer() -> u32 { 42 }
///
/// #[derive(Default, Mock, CallReal)]
/// struct MyMock {
///     // mock state...
///     _switch: RealCallSwitch,
/// }
///
/// impl MyMock {
///     fn answer(&self) -> u32 {
///         let _guard = self.call_real();
///         let real_answer = self.call_real_once().scope(answer);
///         // ^ will panic here: there is an alive call switch guard
///         real_answer + 1
///     }
/// }
///
/// let _guard = MyMock::default().set_as_mock();
/// answer(); // triggers the panic
/// ```
// Unfortunately, we cannot define `call_real(&mut self, ..)` to move guard checks
// to compile time; we only have a shared ref to the mock state.
pub trait CallReal {
    /// Returns a reference to the call switch.
    #[doc(hidden)] // low-level implementation detail
    fn access_switch<R>(&self, action: impl FnOnce(&RealCallSwitch) -> R) -> R;

    /// Delegates all calls to the mocked functions / methods to the real implementation until
    /// the returned [`RealCallGuard`] is dropped.
    ///
    /// # Panics
    ///
    /// Panics if the real / mock implementation switch is already set to "real"
    /// (e.g., there is an alive guard produced by an earlier call to [`Self::call_real()`]).
    /// This may lead to unexpected switch value for the further calls and is thus prohibited.
    fn call_real(&self) -> RealCallGuard<'_, Self> {
        <Self as CallReal>::access_switch(self, |switch| {
            switch.assert_inactive();
            switch.0.set(RealCallMode::Always);
        });
        RealCallGuard { controller: self }
    }

    /// Delegates the first call to the mocked functions / methods to the real implementation until
    /// the returned [`RealCallGuard`] is dropped. Further calls will be directed to the mock.
    ///
    /// # Panics
    ///
    /// Panics under the same circumstances as [`Self::call_real()`].
    fn call_real_once(&self) -> RealCallGuard<'_, Self> {
        <Self as CallReal>::access_switch(self, |switch| {
            switch.assert_inactive();
            switch.0.set(RealCallMode::Once);
        });
        RealCallGuard { controller: self }
    }
}

impl<T: CallReal> CheckRealCall for T {
    fn should_call_real(&self) -> bool {
        self.access_switch(RealCallSwitch::should_delegate)
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
///         self.call_real().scope(|| { /* ... */ });
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

    fn assert_inactive(&self) {
        assert_eq!(
            self.0.get(),
            RealCallMode::Inactive,
            "Real / mock switch is set to \"real\" when `call_real()` or `call_real_once()` \
             is called. This may lead to unexpected switch value for the further calls \
             and is thus prohibited"
        );
    }
}

/// Guard for the real / mock implementation switch.
///
/// `RealCallGuard`s are produced by the methods in the [`CallReal`] trait; see its docs
/// for more details.
#[derive(Debug)]
#[must_use = "If unused, the guard won't affect any calls"]
pub struct RealCallGuard<'a, T: CallReal + ?Sized> {
    controller: &'a T,
}

impl<T: CallReal + ?Sized> Drop for RealCallGuard<'_, T> {
    fn drop(&mut self) {
        self.controller.access_switch(|switch| {
            switch.0.set(RealCallMode::Inactive);
        });
    }
}

impl<T: CallReal + ?Sized> RealCallGuard<'_, T> {
    /// Executes the provided closure under this guard and then drops it.
    pub fn scope<R>(self, action: impl FnOnce() -> R) -> R {
        let result = action();
        drop(self);
        result
    }

    /// Executes the provided future under this guard and then drops it.
    pub async fn async_scope<Fut: Future>(self, action: Fut) -> Fut::Output {
        let result = action.await;
        drop(self);
        result
    }
}

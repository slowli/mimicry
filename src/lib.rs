use once_cell::sync::OnceCell;

mod shared;
mod tls;

pub use crate::{
    shared::{Shared, SharedGuard, SharedRef},
    tls::{ThreadLocal, ThreadLocalGuard, ThreadLocalRef},
};
pub use mock_derive::{mock, Mock};

use core::{
    cell::{Cell, RefCell, RefMut},
    ops,
};

pub trait HandleMock<'a, T> {
    #[doc(hidden)] // only used in macros
    type Ref: CallMock<T> + 'a;
    type Guard: 'a;

    /// Returns a reference to the shared mock state, or `None` if the mock is not set.
    #[doc(hidden)] // only used in macros
    fn get(&'a self) -> Option<Self::Ref>;

    /// Returns an exclusive guard to the shared state. Until this guard is dropped, other
    /// threads will block on [`Self::set()`].
    fn set(&'a self, state: T) -> Self::Guard;

    fn set_default(&'a self) -> Self::Guard
    where
        T: Default,
    {
        self.set(T::default())
    }
}

#[doc(hidden)] // only used in macros
pub trait CallMock<T> {
    fn call_mock<R>(self, switch: &FallbackSwitch, action: impl FnOnce(Context<'_, T>) -> R) -> R;
}

/// Wrapper that allows creating `static` instances of `Share` impls.
#[derive(Debug)]
pub struct Static<T> {
    cell: OnceCell<T>,
}

impl<T> Static<T> {
    pub const fn new() -> Self {
        Self {
            cell: OnceCell::new(),
        }
    }
}

impl<'a, T, S> HandleMock<'a, T> for Static<S>
where
    S: HandleMock<'a, T> + Default,
{
    type Ref = S::Ref;
    type Guard = S::Guard;

    fn get(&'a self) -> Option<Self::Ref> {
        let cell = self.cell.get_or_init(S::default);
        cell.get()
    }

    fn set(&'a self, state: T) -> Self::Guard {
        let cell = self.cell.get_or_init(S::default);
        cell.set(state)
    }
}

pub trait Mock: Sized {
    type Shared: for<'a> HandleMock<'a, Self> + 'static + Send + Sync;

    fn instance() -> &'static Static<Self::Shared>;
}

/// Context with and access to the mock state and fallbacks for mocked functions.
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

    /// Returns an exclusive reference to the mock state.
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

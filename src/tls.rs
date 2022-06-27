//! Thread-local implementation of `HandleMock`.

use core::cell::{RefCell, RefMut};

use crate::{CallMock, Context, FallbackSwitch, GetMock, SetMock};

/// Thread-local mock state wrapper.
///
/// This type rarely needs to be used directly; `#[derive(Mock)]` macro with the default settings
/// sets a wrapper automatically.
///
/// As the name implies, this wrapper does not share the mock state across threads. If a thread
/// is spawned during test, mocked functions called from this thread will always use the real
/// implementations. This behavior is fine in simple cases, i.e., unless mocked functions
/// are called from multiple threads spawned by a single test. If cross-thread mocking is required,
/// consider [`Shared`](crate::Shared) wrapper.
///
/// # Examples
///
/// ```
/// use mimicry::{Mock, SetMock};
///
/// #[derive(Debug, Default, Mock)]
/// struct MockState {
///     // fields to support mock logic
/// }
///
/// #[test]
/// fn some_test() {
///     // Sets the mock state until `mock_guard` is dropped.
///     let mock_guard = MockState::instance().set_default();
///     // Call mocked functions (maybe, indirectly). All calls
///     // need to happen from the original test thread.
///     let state = mock_guard.into_inner();
///     // Can check the state here...
/// }
/// ```
#[derive(Debug)]
pub struct ThreadLocal<T: Send> {
    tls: thread_local::ThreadLocal<ThreadLocalInner<T>>,
}

impl<T: Send> Default for ThreadLocal<T> {
    fn default() -> Self {
        Self {
            tls: thread_local::ThreadLocal::new(),
        }
    }
}

#[derive(Debug)]
struct ThreadLocalInner<T> {
    inner: RefCell<Option<T>>,
    write_lock: RefCell<()>,
}

impl<T> Default for ThreadLocalInner<T> {
    fn default() -> Self {
        Self {
            inner: RefCell::new(None),
            write_lock: RefCell::new(()),
        }
    }
}

impl<'a, T: Send + 'static> GetMock<'a, T> for ThreadLocal<T> {
    type Ref = ThreadLocalRef<'a, T>;

    fn get(&'a self) -> Option<ThreadLocalRef<'_, T>> {
        let cell = self.tls.get_or_default();
        if cell.inner.borrow().is_some() {
            Some(ThreadLocalRef { guard: &cell.inner })
        } else {
            None
        }
    }
}

impl<'a, T: Send + 'static> SetMock<'a, T> for ThreadLocal<T> {
    type Guard = ThreadLocalGuard<'a, T>;

    fn set(&self, state: T) -> ThreadLocalGuard<'_, T> {
        let cell = self.tls.get_or_default();
        let guard = cell.write_lock.try_borrow_mut().unwrap_or_else(|_| {
            panic!("cannot set mock state while the previous state is active");
        });
        *cell.inner.borrow_mut() = Some(state);

        ThreadLocalGuard {
            mock: &cell.inner,
            _guard: guard,
        }
    }
}

#[derive(Debug)]
#[doc(hidden)] // only (indirectly) used in macros
pub struct ThreadLocalRef<'a, T> {
    guard: &'a RefCell<Option<T>>,
}

impl<T: 'static + Send> CallMock<T> for ThreadLocalRef<'_, T> {
    fn call_mock<R>(self, switch: &FallbackSwitch, action: impl FnOnce(Context<'_, T>) -> R) -> R {
        action(Context::new(self.guard, switch))
    }
}

/// Exclusive guard on a [`ThreadLocal`] mock.
///
/// This guard is mostly useful for mock state manipulation; unlike
/// [`SharedGuard`](crate::SharedGuard), it does not provide meaningful synchronization.
/// If [`SetMock::set()`] is called on a thread that has an active guard, such a call will
/// panic; calls on other threads (i.e., in tests running concurrently) are not affected.
#[derive(Debug)]
pub struct ThreadLocalGuard<'a, T> {
    mock: &'a RefCell<Option<T>>,
    _guard: RefMut<'a, ()>,
}

impl<T> Drop for ThreadLocalGuard<'_, T> {
    fn drop(&mut self) {
        self.mock.borrow_mut().take();
    }
}

impl<T> ThreadLocalGuard<'_, T> {
    /// Performs an action on the mock state without releasing the guard. This can be used
    /// to adjust the mock state, check or take some parts of it (such as responses).
    #[allow(clippy::missing_panics_doc)] // unwrap() is safe by construction
    pub fn with<R>(&mut self, action: impl FnOnce(&mut T) -> R) -> R {
        action(self.mock.borrow_mut().as_mut().unwrap())
    }

    /// Consumes this guard and returns the mock state.
    #[allow(clippy::missing_panics_doc)] // unwrap() is safe by construction
    pub fn into_inner(self) -> T {
        self.mock.borrow_mut().take().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Static;

    use static_assertions::assert_impl_all;
    use std::cell::Cell;

    assert_impl_all!(ThreadLocal<Cell<u8>>: Send, Sync);
    assert_impl_all!(Static<ThreadLocal<Cell<u8>>>: Send, Sync);
}

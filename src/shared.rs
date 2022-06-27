//! Thread-safe implementation of `HandleMock`.

use parking_lot::{Mutex, MutexGuard, ReentrantMutex, ReentrantMutexGuard};

use core::cell::RefCell;

use crate::{CallMock, Context, FallbackSwitch, GetMock, LockMock, SetMock};

/// Wrapper around [`Mock`](crate::Mock) state that provides cross-thread synchronization.
///
/// This type rarely needs to be used directly; `#[derive(Mock)]` macro with a `#[mock(shared)]`
/// attribute on the container will set it up automatically.
///
/// Unlike [`ThreadLocal`](crate::ThreadLocal) wrapper, this one shares the state across
/// threads, with state synchronization via reentrant mutexes (to allow for recursive calls).
/// Setting the state is synchronized via a mutex as well: while one test thread
/// has a [`SharedGuard`], other tests attempting to set the state will block.
///
/// # Pitfalls
///
/// Tests that do not set the mock state (i.e., ones that want to deal with real implementations)
/// can still observe a mock impl "spilled" from another test. This is most probably not what
/// you want, and there are ways to deal with this issue:
///
/// - Run tests one at a time via `cargo test -j 1`.
/// - Call [`LockMock::lock()`] at the beginning of the relevant tests.
///
/// # Examples
///
/// ```
/// use mimicry::{mock, Context, Mock, SetMock};
/// # use std::{collections::HashSet, thread};
///
/// #[derive(Debug, Default, Mock)]
/// #[mock(shared)]
/// // ^ use the `Shared` wrapper instead of the default thread-local one
/// struct MockState {
///     counter: u32,
/// }
///
/// impl MockState {
///     fn mock_answer(mut ctx: Context<'_, Self>) -> u32 {
///         let counter = ctx.state().counter;
///         ctx.state().counter += 1;
///         counter
///     }
/// }
///
/// // Mocked function.
/// #[mock(using = "MockState")]
/// fn answer() -> u32 { 42 }
///
/// #[test]
/// # fn catch() {}
/// fn some_test() {
///     // Sets the mock state until `mock_guard` is dropped.
///     let mock_guard = MockState::instance().set_default();
///     // Call mocked functions (maybe, indirectly). Calls may originate
///     // from different threads.
///     let threads: Vec<_> = (0..5).map(|_| thread::spawn(answer)).collect();
///     let answers: HashSet<_> = threads
///         .into_iter()
///         .map(|handle| handle.join().unwrap())
///         .collect();
///     assert_eq!(answers, HashSet::from_iter(0..5));
///
///     let state = mock_guard.into_inner();
///     // Can check the state here...
///     assert_eq!(state.counter, 5);
/// }
/// # some_test();
/// ```
#[derive(Debug)]
pub struct Shared<T> {
    inner: ReentrantMutex<RefCell<Option<T>>>,
    write_lock: Mutex<()>,
}

impl<T> Default for Shared<T> {
    fn default() -> Self {
        Self {
            inner: ReentrantMutex::new(RefCell::new(None)),
            write_lock: Mutex::new(()),
        }
    }
}

impl<T> Shared<T> {
    fn lock(&self) -> ReentrantMutexGuard<'_, RefCell<Option<T>>> {
        self.inner.lock()
    }
}

impl<'a, T: 'static> GetMock<'a, T> for Shared<T> {
    type Ref = SharedRef<'a, T>;

    fn get(&self) -> Option<SharedRef<'_, T>> {
        let guard = self.lock();
        if guard.borrow().is_some() {
            Some(SharedRef { guard })
        } else {
            None
        }
    }
}

impl<'a, T: 'static> SetMock<'a, T> for Shared<T> {
    type Guard = SharedGuard<'a, T>;

    fn set(&self, state: T) -> SharedGuard<'_, T> {
        let guard = self.write_lock.lock();
        *self.lock().borrow_mut() = Some(state);

        SharedGuard {
            _guard: guard,
            mock: self,
        }
    }
}

impl<'a, T: 'static> LockMock<'a, T> for Shared<T> {
    type EmptyGuard = MutexGuard<'a, ()>;

    fn lock(&'a self) -> Self::EmptyGuard {
        self.write_lock.lock()
    }
}

/// Shared reference to mock state.
#[derive(Debug)]
#[doc(hidden)] // only (indirectly) used in macros
pub struct SharedRef<'a, T> {
    // Invariant: the `Option` is always `Some(_)`
    guard: ReentrantMutexGuard<'a, RefCell<Option<T>>>,
}

impl<T: 'static> CallMock<T> for SharedRef<'_, T> {
    fn call_mock<R>(self, switch: &FallbackSwitch, action: impl FnOnce(Context<'_, T>) -> R) -> R {
        let state = &*self.guard;
        action(Context::new(state, switch))
    }
}

/// Exclusive lock on the [`Shared`] mock state.
#[derive(Debug)]
pub struct SharedGuard<'a, T> {
    mock: &'a Shared<T>,
    _guard: MutexGuard<'a, ()>,
}

impl<T> SharedGuard<'_, T> {
    /// Returns the enclosed mock state and lifts the exclusive lock.
    #[allow(clippy::missing_panics_doc)] // unwrap() is safe by construction
    pub fn into_inner(self) -> T {
        self.mock.lock().take().unwrap()
    }
}

impl<T> Drop for SharedGuard<'_, T> {
    fn drop(&mut self) {
        self.mock.lock().take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Static;

    use static_assertions::assert_impl_all;

    assert_impl_all!(Shared<()>: Send, Sync);
    assert_impl_all!(Static<Shared<()>>: Send, Sync);
}

//! Thread-safe mock state wrapper.

use ouroboros::self_referencing;
use parking_lot::{Mutex, MutexGuard, ReentrantMutex, ReentrantMutexGuard};

use core::{
    cell::{Ref, RefCell},
    ops,
};

use crate::{GetMock, Guard, LockMock, SetMock};

/// Wrapper around [`Mock`](crate::Mock) state that provides cross-thread synchronization.
///
/// This type rarely needs to be used directly; `#[derive(Mock)]` macro with a `#[mock(shared)]`
/// attribute on the container will set it up automatically.
///
/// Unlike [`ThreadLocal`](crate::ThreadLocal) wrapper, this one shares the state across
/// threads, with state synchronization via reentrant mutexes (to allow for recursive calls).
/// Setting the state is synchronized via a mutex as well: while one test thread
/// has a [`MockGuard`](crate::MockGuard), other tests attempting to set the state will block.
///
/// # Pitfalls
///
/// Tests that do not set the mock state (i.e., ones that want to deal with real implementations)
/// can still observe a mock impl "spilled" from another test. This is most probably not what
/// you want, and there are ways to deal with this issue:
///
/// - Run tests one at a time via `cargo test -j 1`.
/// - Call [`Mock::lock()`](crate::Mock::lock()) at the beginning of the relevant tests.
///
/// # Examples
///
/// ```
/// use mimicry::{mock, CheckRealCall, Mock};
/// # use std::{collections::HashSet, sync::atomic::{AtomicU32, Ordering}, thread};
///
/// #[derive(Debug, Default, Mock)]
/// #[mock(shared)]
/// // ^ use the `Shared` wrapper instead of the default thread-local one
/// struct MockState {
///     counter: AtomicU32,
/// }
///
/// # impl CheckRealCall for MockState {}
/// impl MockState {
///     fn answer(&self) -> u32 {
///         self.counter.fetch_add(1, Ordering::Relaxed)
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
///     let mock_guard = MockState::default().set_as_mock();
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
///     assert_eq!(state.counter.into_inner(), 5);
/// }
/// # some_test();
/// ```
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "shared")))]
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
            Some(SharedRef::from_guard(guard))
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
#[self_referencing]
pub struct SharedRef<'a, T> {
    guard: ReentrantMutexGuard<'a, RefCell<Option<T>>>,
    #[borrows(guard)]
    #[covariant]
    state: Ref<'this, T>,
}

impl<T> ops::Deref for SharedRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.borrow_state()
    }
}

impl<'a, T> SharedRef<'a, T> {
    fn from_guard(guard: ReentrantMutexGuard<'a, RefCell<Option<T>>>) -> Self {
        SharedRefBuilder {
            guard,
            state_builder: |guard| Ref::map(guard.borrow(), |option| option.as_ref().unwrap()),
        }
        .build()
    }
}

/// Exclusive lock on the [`Shared`] mock state.
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "shared")))]
pub struct SharedGuard<'a, T> {
    mock: &'a Shared<T>,
    _guard: MutexGuard<'a, ()>,
}

impl<T: 'static> Guard<T> for SharedGuard<'_, T> {
    fn with<R>(&mut self, action: impl FnOnce(&mut T) -> R) -> R {
        let locked = self.mock.lock();
        let mut borrowed = locked.borrow_mut();
        action(borrowed.as_mut().unwrap())
    }

    fn into_inner(self) -> T {
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

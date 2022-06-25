//! Thread-safe implementation of `HandleMock`.

use parking_lot::{Mutex, MutexGuard, ReentrantMutex, ReentrantMutexGuard};

use core::cell::RefCell;

use crate::{CallMock, Context, FallbackSwitch, HandleMock};

/// Wrapper around [`Mock`] state that provides cross-thread synchronization.
// FIXME: large issue is that unrelated threads can get mock values
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

impl<'a, T: 'static + Send + Sync> HandleMock<'a, T> for Shared<T> {
    type Ref = SharedRef<'a, T>;
    type Guard = SharedGuard<'a, T>;

    fn get(&self) -> Option<SharedRef<'_, T>> {
        let guard = self.lock();
        if guard.borrow().is_some() {
            Some(SharedRef { guard })
        } else {
            None
        }
    }

    fn set(&self, state: T) -> SharedGuard<'_, T> {
        let guard = self.write_lock.lock();
        *self.lock().borrow_mut() = Some(state);

        SharedGuard {
            _guard: guard,
            mock: self,
        }
    }
}

/// Shared reference to mock state.
#[derive(Debug)]
pub struct SharedRef<'a, T> {
    // Invariant: the `Option` is always `Some(_)`
    guard: ReentrantMutexGuard<'a, RefCell<Option<T>>>,
}

impl<T: 'static + Send + Sync> CallMock<T> for SharedRef<'_, T> {
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
    pub fn into_inner(self) -> T {
        self.mock.lock().take().unwrap()
        // ^ unwrap() should be safe by construction
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

//! Thread-local implementation of `HandleMock`.

use core::cell::{RefCell, RefMut};

use crate::{CallMock, Context, FallbackSwitch, HandleMock};

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

impl<'a, T: Send + 'static> HandleMock<'a, T> for ThreadLocal<T> {
    type Ref = ThreadLocalRef<'a, T>;
    type Guard = ThreadLocalGuard<'a, T>;

    fn get(&'a self) -> Option<ThreadLocalRef<'_, T>> {
        let cell = self.tls.get_or_default();
        if cell.inner.borrow().is_some() {
            Some(ThreadLocalRef { guard: &cell.inner })
        } else {
            None
        }
    }

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
pub struct ThreadLocalRef<'a, T> {
    guard: &'a RefCell<Option<T>>,
}

impl<T: 'static + Send> CallMock<T> for ThreadLocalRef<'_, T> {
    fn call_mock<R>(self, switch: &FallbackSwitch, action: impl FnOnce(Context<'_, T>) -> R) -> R {
        action(Context::new(self.guard, switch))
    }
}

/// Exclusive lock on the [`Shared`] mock state.
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

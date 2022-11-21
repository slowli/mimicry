//! Answers for mocks.

use parking_lot::Mutex;

use core::{fmt, future::Future, iter, mem};
use std::{sync::Arc, thread};

/// Answers for a function call.
///
/// `Answers` are similar to an [`Iterator`], but with some additional functionality:
///
/// - Response can be based on a certain *context* (the second type param) provided to
///   [`Self::next_for()`].
/// - The provided contexts are recorded for each call and then can be retrieved using
///   [`Self::take_calls()`]. This can be used to verify calls.
///
/// The intended usage of `Answers` is as an element of [`Mock`](crate::Mock) state
/// used in one or more mock methods.
///
/// # Examples
///
/// ```
/// # use mimicry::Answers;
/// let mut answers: Answers<usize> = Answers::from_values([1, 3, 5]);
/// let value: usize = answers.next_for(());
/// assert_eq!(value, 1);
/// assert_eq!(answers.next_for(()), 3);
/// assert_eq!(answers.take_calls().len(), 2);
/// ```
///
/// Context-dependent `Answers`:
///
/// ```
/// # use mimicry::Answers;
/// let mut counter = 0;
/// let mut answers = Answers::from_fn(move |s: &String| {
///     if counter == 0 && s == "test" {
///         counter += 1;
///         42
///     } else {
///         s.len()
///     }
/// });
/// assert_eq!(answers.next_for("test".into()), 42);
/// assert_eq!(answers.next_for("??".into()), 2);
/// assert_eq!(answers.next_for("test".into()), 4);
///
/// let calls = answers.take_calls();
/// assert_eq!(calls, ["test", "??", "test"]);
/// ```
///
/// ## Channels
///
/// Basic usage of [`Answers::channel()`]:
///
/// ```
/// use mimicry::Answers;
///
/// let (mut answers, mut sx) = Answers::channel();
/// // `rx` can be placed in the mock state
/// sx.send(42).scope(|| {
///     // Realistically, here you would call something that uses
///     // the mock, possibly with indirection.
///     assert_eq!(answers.next_for(()), 42);
/// });
/// ```
///
/// Async scope wrapper is also available:
///
/// ```
/// # use mimicry::Answers;
/// # async fn test_wrapper() {
/// let (mut answers, mut sx) = Answers::channel();
/// let future = sx.send(42).async_scope(async {
///     assert_eq!(answers.next_for(()), 42);
/// });
/// future.await;
/// # }
/// ```
///
/// More advanced usage with explicit [guard](AnswersGuard) handling:
///
/// ```
/// # use mimicry::{Answers, AnswersGuard};
/// let (mut answers, mut sx) = Answers::channel();
/// let guard: AnswersGuard<_> = sx.send_all([0, 1, 2, 0, 1]);
/// for i in 0..4 {
///     assert_eq!(answers.next_for(i), i % 3);
/// }
/// guard.discard(); // ignore the remaining answer
/// ```
///
/// If not all answers were used when a guard is dropped, it panics:
///
/// ```should_panic
/// # use mimicry::Answers;
/// let (mut answers, mut sx) = Answers::channel();
/// sx.send_all([0, 1]).scope(|| {
///     assert_eq!(answers.next_for(()), 0);
///     // The code under test should make another call to the mock,
///     // but it does not.
/// });
/// ```
///
/// ## Functional values
///
/// To deal with more complex cases, `Answers` can contain functional values.
///
/// ```
/// # use mimicry::{mock, Answers, Mock, Mut};
/// #[mock(using = "SimpleMock::mock_fn")]
/// fn tested_fn(s: &str, start: usize) -> &str {
///     &s[start..]
/// }
///
/// type StrFn = fn(&str) -> &str;
///
/// #[derive(Mock)]
/// #[mock(mut)]
/// struct SimpleMock {
///     str_fns: Answers<StrFn, (String, usize)>,
/// }
///
/// impl SimpleMock {
///     fn mock_fn<'s>(this: &Mut<Self>, s: &'s str, start: usize) -> &'s str {
///         let context = (s.to_owned(), start);
///         let str_fn = this.borrow().str_fns.next_for(context);
///         str_fn(s)
///     }
/// }
///
/// // Setup mock with 2 functions.
/// let return_test: StrFn = |_| "test";
/// let suffix: StrFn = |s| &s[1..];
/// let mock = SimpleMock {
///     str_fns: Answers::from_values([return_test, suffix]),
/// };
/// let guard = mock.set_as_mock();
///
/// // Perform some tests.
/// assert_eq!(tested_fn("first", 0), "test");
/// assert_eq!(tested_fn("second", 3), "econd");
///
/// // Verify mock calls.
/// let calls = guard.into_inner().str_fns.take_calls();
/// assert_eq!(calls.len(), 2);
/// assert_eq!(calls[0].0, "first");
/// assert_eq!(calls[1].1, 3);
/// ```
pub struct Answers<V, Ctx = ()> {
    inner: Box<dyn FnMut(&Ctx) -> V + Send>,
    calls: Vec<Ctx>,
}

impl<V, Ctx: fmt::Debug> fmt::Debug for Answers<V, Ctx> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Answers")
            .field("calls", &self.calls)
            .finish()
    }
}

impl<V, Ctx> Default for Answers<V, Ctx> {
    fn default() -> Self {
        Self::from_fn(|_| panic!("no answers provided"))
    }
}

impl<V, Ctx> Answers<V, Ctx> {
    /// Answers based on the provided function.
    pub fn from_fn<F>(function: F) -> Self
    where
        F: FnMut(&Ctx) -> V + Send + 'static,
    {
        Self {
            inner: Box::new(function),
            calls: Vec::new(),
        }
    }

    /// Answers with values from the provided iterator. Once the iterator runs out of items,
    /// panics.
    pub fn from_values<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = V>,
        I::IntoIter: Send + 'static,
    {
        let mut iter = iter.into_iter();
        Self::from_fn(move |_| iter.next().expect("run out of mock responses"))
    }

    /// Selects an answer based on the specified `context`. The context is recorded and can
    /// then be retrieved via [`Self::take_calls()`].
    pub fn next_for(&mut self, context: Ctx) -> V {
        let response = (self.inner)(&context);
        self.calls.push(context);
        response
    }

    /// Takes contexts for recorded calls since the last call to [`Self::take_calls()`],
    /// or after creation if called for the first time.
    pub fn take_calls(&mut self) -> Vec<Ctx> {
        mem::take(&mut self.calls)
    }
}

impl<V: Send + 'static, Ctx> Answers<V, Ctx> {
    /// Answers with the provided `value` once. Further calls will panic.
    pub fn from_value_once(value: V) -> Self {
        Self::from_values(iter::once(value))
    }

    /// Creates a new `Answers` instance that can receive answers dynamically via a channel.
    /// The channel functions similar to a [blocking channel](std::sync::mpsc)
    /// from the standard library.
    ///
    /// Unlike with [`Self::from_value()`] / [`Self::from_values()`], using a channel allows
    /// building answers dynamically after the mock is already set up.
    #[allow(clippy::missing_panics_doc)] // false positive
    pub fn channel() -> (Self, AnswersSender<V>) {
        let channel = Arc::new(Mutex::new(AnswersChannel {
            answers: Vec::new(),
        }));
        let sender = AnswersSender {
            inner: Arc::clone(&channel),
        };
        let this = Self::from_fn(move |_| {
            let mut guard = channel.lock();
            guard.answers.pop().unwrap_or_else(|| {
                panic!("no answer provided for call");
            })
        });
        (this, sender)
    }
}

impl<V: Clone + Send + 'static, Ctx> Answers<V, Ctx> {
    /// Answers with the provided `value` infinite number of times.
    pub fn from_value(value: V) -> Self {
        Self::from_values(iter::repeat(value))
    }
}

#[derive(Debug)]
struct AnswersChannel<V> {
    answers: Vec<V>,
}

/// Sender part of a channel created by [`Answers::channel()`].
///
/// # Examples
///
/// See [`Answers`](Answers#channels) for examples of usage.
#[derive(Debug)]
pub struct AnswersSender<V> {
    inner: Arc<Mutex<AnswersChannel<V>>>,
}

impl<V> AnswersSender<V> {
    /// Sends a single value over the channel. The value will be used as the next answer.
    ///
    /// # Return value
    ///
    /// Returns a guard that will automatically check that the value has been used
    /// when going out of scope.
    pub fn send(&mut self, value: V) -> AnswersGuard<'_, V> {
        self.send_all([value])
    }

    /// Sends several values over the channel. The values will be used as answers in the same order
    /// as returned by the iterator.
    ///
    /// # Return value
    ///
    /// Returns a guard that will automatically check that all the values have been used
    /// when going out of scope.
    pub fn send_all(&mut self, values: impl IntoIterator<Item = V>) -> AnswersGuard<'_, V> {
        let mut values: Vec<_> = values.into_iter().collect();
        values.reverse();
        *self.inner.lock() = AnswersChannel { answers: values };
        AnswersGuard {
            inner: &mut self.inner,
        }
    }
}

/// Guard ensuring that answers sent from an [`AnswersSender`] are timely consumed.
///
/// The consumption check is performed on guard drop: either implicit, or explicit
/// via [`Self::scope()`].
#[derive(Debug)]
#[must_use = "If not used, the answer value(s) will be immediately discarded"]
pub struct AnswersGuard<'a, V> {
    inner: &'a mut Arc<Mutex<AnswersChannel<V>>>,
}

impl<V> AnswersGuard<'_, V> {
    /// Executes the provided closure and checks that all the answers were consumed by it.
    pub fn scope<R>(self, action: impl FnOnce() -> R) -> R {
        let result = action();
        drop(self);
        result
    }

    /// Executes the provided future and checks that all the answers were consumed by it.
    ///
    /// While [`Self::scope()`]`.await` with a closure that returns a future technically works,
    /// it will probably result in a failed consumption check. Indeed, the scope
    /// will be exited and the guard dropped before the future is polled.
    pub async fn async_scope<Fut: Future>(self, action: Fut) -> Fut::Output {
        let result = action.await;
        drop(self);
        result
    }

    /// Drops this guard discarding any remaining answers, so that the guard does not panic.
    pub fn discard(self) {
        self.inner.lock().answers.clear();
    }
}

impl<V> Drop for AnswersGuard<'_, V> {
    fn drop(&mut self) {
        if !thread::panicking() {
            let guard = self.inner.lock();
            assert!(
                guard.answers.is_empty(),
                "{} answer(s) not consumed from answers channel",
                guard.answers.len()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_basics() {
        let mut answers: Answers<i32> = Answers::from_values([1, 2, 3, 5]);
        assert_eq!(answers.next_for(()), 1);
        assert_eq!(answers.next_for(()), 2);
        assert_eq!(answers.next_for(()), 3);
        assert_eq!(answers.next_for(()), 5);
        let calls = answers.take_calls();
        assert_eq!(calls.len(), 4);
    }

    #[test]
    fn answers_with_context() {
        let mut answers: Answers<usize, String> = Answers::from_values(5..10);
        let samples = ["test", "various", "strings"];
        for (i, s) in samples.into_iter().enumerate() {
            assert_eq!(answers.next_for(s.to_owned()), i + 5);
        }
        let calls = answers.take_calls();
        assert_eq!(calls, samples);

        let mut counter = 0;
        let mut answers: Answers<usize, String> = Answers::from_fn(move |s: &String| {
            counter += 1;
            match s.as_str() {
                "test" => 42,
                _ if counter < 3 => s.len(),
                _ => 0,
            }
        });
        let real_answers: Vec<_> = samples
            .into_iter()
            .map(|s| answers.next_for(s.to_owned()))
            .collect();
        assert_eq!(real_answers, [42, 7, 0]);
        let calls = answers.take_calls();
        assert_eq!(calls, samples);
    }

    fn assert_static<T: 'static>(value: T) -> T {
        value
    }

    #[test]
    fn function_answers() {
        type LenFn = fn(&str) -> usize;

        let test_fn: LenFn = |s| usize::from(s == "test");
        let fns = iter::repeat(str::len as LenFn).take(2).chain([test_fn]);
        let answers: Answers<LenFn> = Answers::from_values(fns);
        let mut answers = assert_static(answers);
        assert_eq!(answers.next_for(())("test"), 4);
        assert_eq!(answers.next_for(())("test"), 4);
        assert_eq!(answers.next_for(())("test"), 1);
    }

    #[test]
    fn answers_channel_basics() {
        let (mut answers, mut sx) = Answers::channel();
        {
            let _guard = sx.send(42);
            assert_eq!(answers.next_for("test"), 42);
            assert_eq!(answers.take_calls(), ["test"]);
        }
        sx.send(1).scope(|| {
            assert_eq!(answers.next_for("test"), 1);
        });

        let _guard = sx.send_all([555, 777]);
        assert_eq!(answers.next_for("foo"), 555);
        assert_eq!(answers.next_for("bar"), 777);
    }

    #[test]
    #[should_panic(expected = "1 answer(s) not consumed")]
    fn partially_consumed_answers_channel() {
        let (mut answers, mut sx) = Answers::channel();
        let _guard = sx.send_all([555, 777]);
        assert_eq!(answers.next_for("foo"), 555);
    }
}

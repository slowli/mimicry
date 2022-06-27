use std::{borrow::Borrow, collections::HashMap, hash::Hash, thread};

#[cfg(feature = "shared")]
use mimicry::LockMock;
use mimicry::{mock, Context, Mock, SetMock};

#[test]
fn mock_basics() {
    #[mock(using = "SearchMock")]
    fn search(haystack: &str, needle: char) -> Option<usize> {
        haystack.chars().position(|ch| ch == needle)
    }

    #[derive(Default, Mock)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct SearchMock {
        called_times: usize,
    }

    impl SearchMock {
        fn mock_search(mut cx: Context<'_, Self>, haystack: &str, needle: char) -> Option<usize> {
            cx.state().called_times += 1;
            match haystack {
                "test" => Some(42),
                short if short.len() <= 2 => None,
                _ => cx.fallback(|| search(haystack, if needle == '?' { 'e' } else { needle })),
            }
        }
    }

    let recovered = {
        let guard = SearchMock::instance().set(SearchMock::default());
        assert_eq!(search("test", '?'), Some(42));
        assert_eq!(search("?!", '?'), None);
        assert_eq!(search("needle?", '?'), Some(1));
        assert_eq!(search("needle?", 'd'), Some(3));
        guard.into_inner()
    };
    assert_eq!(recovered.called_times, 4);

    // Mock is not used here.
    assert_eq!(search("test", '?'), None);
    assert_eq!(search("?!", '?'), Some(0));
    assert_eq!(search("needle?", '?'), Some(6));
}

#[test]
fn mock_with_lifetimes() {
    #[mock(using = "TailMock")]
    fn tail(bytes: &mut [u8]) -> Option<&u8> {
        if bytes.is_empty() {
            None
        } else {
            bytes[1..].fill(0);
            Some(&bytes[0])
        }
    }

    #[derive(Default, Mock)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct TailMock;

    impl TailMock {
        fn mock_tail<'a>(mut cx: Context<'_, Self>, bytes: &'a mut [u8]) -> Option<&'a u8> {
            if bytes == b"test" {
                Some(&0)
            } else {
                cx.fallback(|| tail(bytes))
            }
        }
    }

    let mut bytes = *b"test";
    assert_eq!(tail(&mut bytes), Some(&b't'));
    assert_eq!(bytes, *b"t\0\0\0");

    let _guard = TailMock::instance().set_default();
    let mut bytes = *b"test";
    assert_eq!(tail(&mut bytes), Some(&0));
    assert_eq!(bytes, *b"test");
}

#[test]
fn mock_consuming_args() {
    #[mock(using = "ConsumeMock::consume")]
    fn consume(bytes: Vec<u8>) -> Option<String> {
        String::from_utf8(bytes).ok()
    }

    #[derive(Default, Mock)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct ConsumeMock;

    impl ConsumeMock {
        fn consume(mut cx: Context<'_, Self>, bytes: Vec<u8>) -> Option<String> {
            if bytes.is_ascii() {
                Some(String::from("ASCII"))
            } else {
                cx.fallback(|| consume(bytes))
            }
        }
    }

    let _guard = ConsumeMock::instance().set_default();
    let bytes = b"test".to_vec();
    assert_eq!(consume(bytes).unwrap(), "ASCII");
    let bytes = b"\xD0\xBB\xD1\x96\xD0\xBB".to_vec();
    assert_eq!(consume(bytes).unwrap(), "ліл");
    let bytes = vec![255];
    assert!(consume(bytes).is_none());
}

#[test]
fn mock_for_generic_function() {
    #[mock(using = "GenericMock")]
    fn len<T: AsRef<str>>(value: T) -> usize {
        value.as_ref().len()
    }

    #[mock(using = "GenericMock")]
    fn get_key<K, Q: ?Sized>(map: &HashMap<K, usize>, key: &Q) -> usize
    where
        K: Borrow<Q> + Eq + Hash,
        Q: Eq + Hash,
    {
        map.get(key).copied().unwrap_or(0)
    }

    #[derive(Default, Mock)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct GenericMock {
        len_args: Vec<String>,
        get_key_responses: Vec<usize>,
    }

    impl GenericMock {
        fn mock_len(mut cx: Context<'_, Self>, value: impl AsRef<str>) -> usize {
            cx.state().len_args.push(value.as_ref().to_owned());
            cx.fallback(|| len(value))
        }

        fn mock_get_key<K, Q: ?Sized>(
            mut cx: Context<'_, Self>,
            map: &HashMap<K, usize>,
            key: &Q,
        ) -> usize
        where
            K: Borrow<Q> + Eq + Hash,
            Q: Eq + Hash,
        {
            let response = cx.fallback(|| get_key(map, key));
            cx.state().get_key_responses.push(response);
            response
        }
    }

    let guard = GenericMock::instance().set_default();
    assert_eq!(len("value"), 5);
    assert_eq!(len(String::from("test")), 4);
    let mut map = HashMap::new();
    map.insert(String::from("test"), 23);
    map.insert(String::from("42"), 42);
    assert_eq!(get_key(&map, "test"), 23);
    assert_eq!(get_key(&map, "???"), 0);
    assert_eq!(get_key(&map, "42"), 42);

    let mock = guard.into_inner();
    assert_eq!(mock.len_args, ["value", "test"]);
    assert_eq!(mock.get_key_responses, [23, 0, 42]);
}

#[test]
fn mock_in_impl() {
    struct Wrapper<T>(T);

    impl<T: AsRef<str>> Wrapper<T> {
        #[mock(using = "LenMock")]
        fn len(&self) -> usize {
            self.0.as_ref().len()
        }
    }

    impl Wrapper<String> {
        #[mock(using = "LenMock")]
        fn push(&mut self, value: impl AsRef<str>) -> &mut Self {
            self.0.push_str(value.as_ref());
            self
        }
    }

    #[derive(Mock)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct LenMock {
        min_length: usize,
    }

    impl LenMock {
        fn mock_len<T: AsRef<str>>(mut cx: Context<'_, Self>, this: &Wrapper<T>) -> usize {
            if this.0.as_ref() == "test" {
                42
            } else {
                cx.fallback(|| this.len())
            }
        }

        fn mock_push<'a>(
            mut cx: Context<'_, Self>,
            this: &'a mut Wrapper<String>,
            s: impl AsRef<str>,
        ) -> &'a mut Wrapper<String> {
            if s.as_ref().len() < cx.state().min_length {
                this
            } else {
                cx.fallback(|| this.push(s))
            }
        }
    }

    let guard = LenMock::instance().set(LenMock { min_length: 3 });
    assert_eq!(Wrapper("test!").len(), 5);
    assert_eq!(Wrapper("test").len(), 42);
    assert_eq!(Wrapper(String::from("test")).len(), 42);
    assert_eq!(Wrapper("test??").len(), 6);

    let mut wrapper = Wrapper(String::new());
    wrapper.push("??").push("test").push("!").push("...");
    assert_eq!(wrapper.0, "test...");

    drop(guard);
    wrapper.push(":D");
    assert_eq!(wrapper.0, "test...:D");
}

#[test]
fn mock_in_impl_trait() {
    #[derive(Default)]
    struct Flip {
        state: u8,
    }

    impl Iterator for Flip {
        type Item = u8;

        #[mock(using = "IterMock")]
        fn next(&mut self) -> Option<Self::Item> {
            self.state = 1 - self.state;
            Some(self.state)
        }
    }

    struct Const(u8);

    impl Iterator for Const {
        type Item = u8;

        #[mock(using = "IterMock")]
        fn next(&mut self) -> Option<Self::Item> {
            Some(self.0)
        }
    }

    #[derive(Default, Mock)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct IterMock {
        count: usize,
    }

    impl IterMock {
        fn mock_next<I>(mut cx: Context<'_, Self>, _: &mut I) -> Option<u8> {
            let count = cx.state().count;
            cx.state().count += 1;
            u8::try_from(count).ok()
        }
    }

    let mut flip = Flip::default();
    assert_eq!(flip.by_ref().take(5).collect::<Vec<_>>(), [1, 0, 1, 0, 1]);

    let guard = IterMock::instance().set_default();
    assert_eq!(flip.by_ref().take(5).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
    let mut zero = Const(0);
    assert_eq!(zero.by_ref().take(3).collect::<Vec<_>>(), [5, 6, 7]);

    let mut chained = zero.take(2).chain(flip.take(2));
    assert_eq!(chained.by_ref().take(3).collect::<Vec<_>>(), [8, 9, 10]);
    drop(guard);
    assert_eq!(chained.next(), Some(0)); // "real" next value from `flip`
}

#[test]
fn recursive_fn() {
    #[mock(using = "FactorialMock")]
    fn factorial(n: u64, acc: &mut u64) -> u64 {
        if n <= 1 {
            *acc
        } else {
            *acc = acc.overflowing_mul(n).0;
            factorial(n - 1, acc)
        }
    }

    #[derive(Default, Mock)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct FactorialMock {
        fallback_once: bool,
    }

    impl FactorialMock {
        fn mock_factorial(mut cx: Context<'_, Self>, n: u64, acc: &mut u64) -> u64 {
            if n < 5 {
                *acc // finish the recursion early
            } else if cx.state().fallback_once {
                cx.fallback_once(|| factorial(n, acc))
            } else {
                // Fallback should be applied to both calls here
                cx.fallback(|| factorial(n, acc) * factorial(n - 5, &mut 1))
            }
        }
    }

    assert_eq!(factorial(4, &mut 1), 24);

    let guard = FactorialMock::instance().set_default();
    assert_eq!(factorial(4, &mut 1), 1);
    assert_eq!(factorial(5, &mut 1), 120);
    assert_eq!(factorial(10, &mut 1), 435_456_000);
    assert_eq!(factorial(4, &mut 1), 1);

    let mut mock = guard.into_inner();
    mock.fallback_once = true;
    let _guard = FactorialMock::instance().set(mock);
    assert_eq!(factorial(4, &mut 1), 1);
    assert_eq!(factorial(5, &mut 1), 5);
    assert_eq!(factorial(10, &mut 1), 151200);
}

#[cfg(feature = "shared")]
#[test]
#[allow(clippy::needless_collect)] // needed for threads to be spawned concurrently
fn single_shared_mock_in_multi_thread_env() {
    #[mock(using = "ValueMock")]
    fn value() -> u32 {
        0
    }

    #[derive(Default, Mock)]
    #[mock(shared)]
    struct ValueMock(u32);

    impl ValueMock {
        fn mock_value(mut cx: Context<'_, Self>) -> u32 {
            let value = cx.state().0;
            cx.state().0 += 1;
            value
        }
    }

    let guard = ValueMock::instance().set_default();
    let thread_handles: Vec<_> = (0..5)
        .map(|_| thread::spawn(|| (0..10).map(|_| value()).sum::<u32>()))
        .collect();
    let sum = thread_handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .sum::<u32>();
    assert_eq!(sum, 49 * 50 / 2);

    let count = guard.into_inner().0;
    assert_eq!(count, 50);
}

#[test]
#[allow(clippy::needless_collect)] // needed for threads to be spawned concurrently
fn per_thread_mock_in_multi_thread_env() {
    #[mock(using = "ValueMock")]
    fn value() -> u32 {
        0
    }

    #[derive(Default, Mock)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct ValueMock(u32);

    impl ValueMock {
        fn mock_value(mut cx: Context<'_, Self>) -> u32 {
            let value = cx.state().0;
            cx.state().0 += 1;
            value
        }
    }

    let thread_handles: Vec<_> = (0..5)
        .map(|_| {
            thread::spawn(|| {
                let _guard = ValueMock::instance().set_default();
                (0..10).map(|_| value()).collect::<Vec<_>>()
            })
        })
        .collect();
    let ranges = thread_handles
        .into_iter()
        .map(|handle| handle.join().unwrap());
    let expected_range: Vec<_> = (0..10).collect();
    for range in ranges {
        assert_eq!(range, expected_range);
    }
}

#[cfg(feature = "shared")]
#[test]
fn locking_shared_mocks() {
    use std::time::Duration;

    #[mock(using = "ValueMock")]
    fn value() -> u32 {
        0
    }

    #[derive(Mock)]
    #[mock(shared)]
    struct ValueMock(u32);

    impl ValueMock {
        fn mock_value(mut cx: Context<'_, Self>) -> u32 {
            let value = cx.state().0;
            cx.state().0 += 1;
            value
        }
    }

    fn first_test() {
        let _guard = ValueMock::instance().lock();
        for _ in 0..10 {
            assert_eq!(value(), 0);
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn second_test() {
        let _guard = ValueMock::instance().set(ValueMock(42));
        for i in 42..52 {
            assert_eq!(value(), i);
            thread::sleep(Duration::from_millis(1));
        }
    }

    let first_test_handle = thread::spawn(first_test);
    let second_test_handle = thread::spawn(second_test);
    first_test_handle.join().unwrap();
    second_test_handle.join().unwrap();
}

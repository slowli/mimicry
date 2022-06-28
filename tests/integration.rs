use std::{
    borrow::Borrow,
    collections::HashMap,
    hash::Hash,
    mem,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    thread,
};

use mimicry::{mock, CallReal, Mock, Mut, RealCallSwitch};

#[test]
fn mock_basics() {
    #[mock(using = "SearchMock", rename = "mock_{}")]
    fn search(haystack: &str, needle: char) -> Option<usize> {
        haystack.chars().position(|ch| ch == needle)
    }

    #[derive(Default, Mock)]
    #[cfg_attr(feature = "shared", mock(mut, shared))]
    #[cfg_attr(not(feature = "shared"), mock(mut))]
    struct SearchMock {
        called_times: usize,
    }

    impl SearchMock {
        fn mock_search(this: &Mut<Self>, haystack: &str, needle: char) -> Option<usize> {
            this.borrow().called_times += 1;
            match haystack {
                "test" => Some(42),
                short if short.len() <= 2 => None,
                _ => this.call_real(|| search(haystack, if needle == '?' { 'e' } else { needle })),
            }
        }
    }

    let recovered = {
        let guard = SearchMock::set(SearchMock::default());
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

    #[derive(Default, Mock, CallReal)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct TailMock {
        switch: RealCallSwitch,
    }

    impl TailMock {
        fn tail<'a>(&self, bytes: &'a mut [u8]) -> Option<&'a u8> {
            if bytes == b"test" {
                Some(&0)
            } else {
                self.call_real(|| tail(bytes))
            }
        }
    }

    let mut bytes = *b"test";
    assert_eq!(tail(&mut bytes), Some(&b't'));
    assert_eq!(bytes, *b"t\0\0\0");

    let _guard = TailMock::set_default();
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

    #[derive(Default, Mock, CallReal)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct ConsumeMock(RealCallSwitch);

    impl ConsumeMock {
        fn consume(&self, bytes: Vec<u8>) -> Option<String> {
            if bytes.is_ascii() {
                Some(String::from("ASCII"))
            } else {
                self.call_real(|| consume(bytes))
            }
        }
    }

    let _guard = ConsumeMock::set_default();
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
    #[cfg_attr(feature = "shared", mock(mut, shared))]
    #[cfg_attr(not(feature = "shared"), mock(mut))]
    struct GenericMock {
        len_args: Vec<String>,
        get_key_responses: Vec<usize>,
    }

    impl GenericMock {
        fn len(this: &Mut<Self>, value: impl AsRef<str>) -> usize {
            this.borrow().len_args.push(value.as_ref().to_owned());
            this.call_real(|| len(value))
        }

        fn get_key<K, Q: ?Sized>(this: &Mut<Self>, map: &HashMap<K, usize>, key: &Q) -> usize
        where
            K: Borrow<Q> + Eq + Hash,
            Q: Eq + Hash,
        {
            let response = this.call_real(|| get_key(map, key));
            this.borrow().get_key_responses.push(response);
            response
        }
    }

    let guard = GenericMock::set_default();
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
        #[mock(using = "MockState")]
        fn len(&self) -> usize {
            self.0.as_ref().len()
        }
    }

    #[mock(using = "MockState")]
    impl Wrapper<String> {
        fn push(&mut self, value: impl AsRef<str>) -> &mut Self {
            self.0.push_str(value.as_ref());
            self
        }

        #[mock(using = "MockState::mock_take")]
        fn take(&mut self) -> String {
            mem::take(&mut self.0)
        }
    }

    #[derive(Mock, CallReal)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct MockState {
        min_length: usize,
        switch: RealCallSwitch,
    }

    impl MockState {
        fn len<T: AsRef<str>>(&self, wrapper: &Wrapper<T>) -> usize {
            if wrapper.0.as_ref() == "test" {
                42
            } else {
                self.call_real(|| wrapper.len())
            }
        }

        fn push<'a>(
            &self,
            wrapper: &'a mut Wrapper<String>,
            s: impl AsRef<str>,
        ) -> &'a mut Wrapper<String> {
            if s.as_ref().len() < self.min_length {
                wrapper
            } else {
                self.call_real(|| wrapper.push(s))
            }
        }

        fn mock_take(&self, this: &mut Wrapper<String>) -> String {
            this.0.pop().map_or_else(String::new, String::from)
        }
    }

    let guard = MockState::set(MockState {
        min_length: 3,
        switch: RealCallSwitch::default(),
    });
    assert_eq!(Wrapper("test!").len(), 5);
    assert_eq!(Wrapper("test").len(), 42);
    assert_eq!(Wrapper(String::from("test")).len(), 42);
    assert_eq!(Wrapper("test??").len(), 6);

    let mut wrapper = Wrapper(String::new());
    wrapper.push("??").push("test").push("!").push("...");
    assert_eq!(wrapper.0, "test...");

    let taken = wrapper.take();
    assert_eq!(taken, ".");
    assert_eq!(wrapper.0, "test..");

    drop(guard);
    wrapper.push(":D");
    assert_eq!(wrapper.0, "test..:D");
}

#[test]
fn mock_in_impl_trait() {
    #[derive(Default)]
    struct Flip {
        state: u8,
    }

    #[mock(using = "IterMock", rename = "iter_{}")]
    impl Iterator for Flip {
        type Item = u8;

        fn next(&mut self) -> Option<Self::Item> {
            self.state = 1 - self.state;
            Some(self.state)
        }
    }

    struct Const(u8);

    impl Iterator for Const {
        type Item = u8;

        #[mock(using = "IterMock::iter_next")]
        fn next(&mut self) -> Option<Self::Item> {
            Some(self.0)
        }
    }

    #[derive(Default, Mock)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct IterMock {
        count: AtomicU32,
    }

    impl IterMock {
        fn iter_next<I>(&self, _: &mut I) -> Option<u8> {
            let count = self.count.fetch_add(1, Ordering::Relaxed);
            u8::try_from(count).ok()
        }
    }

    impl mimicry::CheckRealCall for IterMock {}

    let mut flip = Flip::default();
    assert_eq!(flip.by_ref().take(5).collect::<Vec<_>>(), [1, 0, 1, 0, 1]);

    let guard = IterMock::set_default();
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

    #[derive(Default, Mock, CallReal)]
    #[cfg_attr(feature = "shared", mock(shared))]
    struct FactorialMock {
        fallback_once: AtomicBool,
        switch: RealCallSwitch,
    }

    impl FactorialMock {
        fn factorial(&self, n: u64, acc: &mut u64) -> u64 {
            if n < 5 {
                *acc // finish the recursion early
            } else if self.fallback_once.load(Ordering::Relaxed) {
                self.call_real_once(|| factorial(n, acc))
            } else {
                // Fallback should be applied to both calls here
                self.call_real(|| factorial(n, acc) * factorial(n - 5, &mut 1))
            }
        }
    }

    assert_eq!(factorial(4, &mut 1), 24);

    let mut guard = FactorialMock::set_default();
    assert_eq!(factorial(4, &mut 1), 1);
    assert_eq!(factorial(5, &mut 1), 120);
    assert_eq!(factorial(10, &mut 1), 435_456_000);
    assert_eq!(factorial(4, &mut 1), 1);

    guard.with(|mock| {
        mock.fallback_once = AtomicBool::new(true);
    });
    assert_eq!(factorial(4, &mut 1), 1);
    assert_eq!(factorial(5, &mut 1), 5);
    assert_eq!(factorial(10, &mut 1), 151200);

    drop(guard);
    assert_eq!(factorial(4, &mut 1), 24);
}

#[derive(Default, Mock)]
#[cfg_attr(feature = "shared", mock(shared))]
struct ValueMock(AtomicU32);

impl ValueMock {
    fn value(&self) -> u32 {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

impl mimicry::CheckRealCall for ValueMock {}

#[mock(using = "ValueMock")]
fn value() -> u32 {
    0
}

#[cfg(feature = "shared")]
#[test]
#[allow(clippy::needless_collect)] // needed for threads to be spawned concurrently
fn single_shared_mock_in_multi_thread_env() {
    let guard = ValueMock::set_default();
    let thread_handles: Vec<_> = (0..5)
        .map(|_| thread::spawn(|| (0..10).map(|_| value()).sum::<u32>()))
        .collect();
    let sum = thread_handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .sum::<u32>();
    assert_eq!(sum, 49 * 50 / 2);

    let count = guard.into_inner().0.into_inner();
    assert_eq!(count, 50);
}

#[test]
#[allow(clippy::needless_collect)] // needed for threads to be spawned concurrently
fn per_thread_mock_in_multi_thread_env() {
    let thread_handles: Vec<_> = (0..5)
        .map(|_| {
            thread::spawn(|| {
                let _guard = ValueMock::set_default();
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

    fn first_test() {
        let _guard = ValueMock::lock();
        for _ in 0..10 {
            assert_eq!(value(), 0);
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn second_test() {
        let _guard = ValueMock::set(ValueMock(42.into()));
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

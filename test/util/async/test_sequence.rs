use syncbox::util::async::{self, Async, Future};

#[test]
pub fn test_sequencing_two_futures() {
    let (c1, f1) = Future::<i32, ()>::pair();
    let (c2, f2) = Future::<i32, ()>::pair();

    let mut seq = async::sequence(vec![f1, f2]);

    c2.complete(123);

    match seq.await() {
        Ok(Some((v, rest))) => {
            assert_eq!(123, v);
            seq = rest;
        }
        v => panic!("unexpected value: {:?}", v),
    }

    c1.complete(234);

    match seq.await() {
        Ok(Some((v, rest))) => {
            assert_eq!(234, v);
            seq = rest;
        }
        v => panic!("unexpected value: {:?}", v),
    }

    assert!(seq.await().unwrap().is_none());
}

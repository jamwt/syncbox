use syncbox::util::async::{self, Async, Future, AsyncError};

#[test]
pub fn test_sequencing_two_futures_on_demand() {
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

#[test]
pub fn test_sequencing_two_futures_up_front() {
    ::env_logger::init().unwrap();

    let (c1, f1) = Future::<i32, ()>::pair();
    let (c2, f2) = Future::<i32, ()>::pair();

    c2.complete(123);
    c1.complete(234);

    let mut seq = async::sequence(vec![f1, f2]);

    match seq.await() {
        Ok(Some((v, rest))) => {
            assert_eq!(123, v);
            seq = rest;
        }
        v => panic!("unexpected value: {:?}", v),
    }

    match seq.await() {
        Ok(Some((v, rest))) => {
            assert_eq!(234, v);
            seq = rest;
        }
        v => panic!("unexpected value: {:?}", v),
    }

    assert!(seq.await().unwrap().is_none());
}

#[test]
pub fn test_sequencing_failed_future() {
    let (c1, f1) = Future::<i32, ()>::pair();
    let (c2, f2) = Future::<i32, ()>::pair();

    let seq = async::sequence(vec![f1, f2]);

    c2.fail(());
    c1.complete(234);

    match seq.await() {
        Err(AsyncError::ExecutionError(_)) => {} // success
        v => panic!("unexpected value {:?}", v),
    }
}

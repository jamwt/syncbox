use super::{receipt, stream, Async, Stream, Cancel, Receipt, AsyncResult, AsyncError};
use super::core::{self, Core};
use std::fmt;

/* TODO:
 * - Add AsyncVal trait that impls all the various monadic fns
 */

#[unsafe_no_drop_flag]
pub struct Future<T: Send, E: Send> {
    core: Option<Core<T, E>>,
}

impl<T: Send, E: Send> Future<T, E> {
    pub fn pair() -> (Complete<T, E>, Future<T, E>) {
        let core = Core::new();
        let future = Future { core: Some(core.clone()) };

        (Complete { core: Some(core) }, future)
    }

    /// Returns a future that will immediately succeed with the supplied value.
    ///
    /// ```
    /// use syncbox::util::async::*;
    ///
    /// Future::<i32, ()>::of(1).and_then(|val| {
    ///     assert!(val == 1);
    ///     Ok(val + 1)
    /// });
    /// ```
    pub fn of(val: T) -> Future<T, E> {
        Future { core: Some(Core::with_value(Ok(val))) }
    }

    /// Returns a future that will immediately fail with the supplied error.
    ///
    /// ```
    /// use syncbox::util::async::*;
    /// use syncbox::util::async::AsyncError::*;
    ///
    /// Future::error("hi").or_else(|err| {
    ///     match err {
    ///         ExecutionError(e) => assert!(e == "hi"),
    ///         CancellationError => unreachable!()
    ///     }
    ///
    ///     Ok(())
    /// });
    /// ```
    pub fn error(err: E) -> Future<T, E> {
        let core = Core::with_value(Err(AsyncError::wrap(err)));
        Future { core: Some(core) }
    }

    /// Returns a future that will immediately be cancelled
    ///
    /// ```
    /// use syncbox::util::async::*;
    /// use syncbox::util::async::AsyncError::*;
    ///
    /// Future::<&'static str, ()>::canceled().or_else(|err| {
    ///     match err {
    ///         ExecutionError(e) => unreachable!(),
    ///         CancellationError => assert!(true)
    ///     }
    ///
    ///     Ok("handled")
    /// });
    /// ```
    pub fn canceled() -> Future<T, E> {
        let core = Core::with_value(Err(AsyncError::canceled()));
        Future { core: Some(core) }
    }

    /// Returns a future that won't kick off its async action until
    /// a consumer registers interest.
    ///
    /// ```
    /// use syncbox::util::async::*;
    ///
    /// let post = Future::lazy(|| {
    ///     // Imagine a call to an HTTP lib, like so:
    ///     // http::get("/posts/1")
    ///     Ok("HTTP response")
    /// });
    ///
    /// // the HTTP request has not happened yet
    ///
    /// // later...
    ///
    /// post.and_then(|p| {
    ///     println!("{:?}", p);
    /// });
    /// // the HTTP request has now happened
    /// ```
    pub fn lazy<F, R>(f: F) -> Future<T, E>
        where F: FnOnce() -> R + Send,
              R: Async<Value=T, Error=E> {

        let (complete, future) = Future::pair();

        // Complete the future with the provided function once consumer
        // interest has been registered.
        complete.receive(move |c: AsyncResult<Complete<T, E>, ()>| {
            if let Ok(c) = c {
                f().receive(move |res| {
                    match res {
                        Ok(v) => c.complete(v),
                        Err(_) => unimplemented!(),
                    }
                });
            }
        });

        future
    }

    /*
     *
     * ===== Computation Builders =====
     *
     */

    pub fn map<F, U>(self, f: F) -> Future<U, E>
        where F: FnOnce(T) -> U + Send,
              U: Send {
        self.and_then(move |val| Ok(f(val)))
    }

    /*
     *
     * ===== Internal Helpers =====
     *
     */

    fn from_core(core: Core<T, E>) -> Future<T, E> {
        Future { core: Some(core) }
    }
}

impl<T: Send + 'static> Future<T, ()> {
    /// Returns a `Future` representing the completion of the given closure.
    /// The closure will be executed on a newly spawned thread.
    ///
    /// ```
    /// use syncbox::util::async::*;
    ///
    /// let future = Future::spawn(|| {
    ///     // Represents an expensive computation
    ///     (0..100).fold(0, |v, i| v + 1)
    /// });
    ///
    /// assert_eq!(100, future.await().unwrap());
    pub fn spawn<F>(f: F) -> Future<T, ()>
        where F: FnOnce() -> T + Send + 'static {

        use std::thread::Thread;
        let (complete, future) = Future::pair();

        // Spawn the thread
        Thread::spawn(move || complete.complete(f()));

        future
    }
}

impl<T: Send, E: Send> Future<Option<(T, Stream<T, E>)>, E> {
    /// An adapter that converts any future into a one-value stream
    pub fn as_stream(mut self) -> Stream<T, E> {
        stream::from_core(core::take(&mut self.core))
    }
}

impl<T: Send, E: Send> Async for Future<T, E> {

    type Value = T;
    type Error = E;
    type Cancel = Receipt<Future<T, E>>;


    fn is_ready(&self) -> bool {
        core::get(&self.core).consumer_is_ready()
    }

    fn is_err(&self) -> bool {
        core::get(&self.core).consumer_is_err()
    }

    fn poll(mut self) -> Result<AsyncResult<T, E>, Future<T, E>> {
        let core = core::take(&mut self.core);

        match core.consumer_poll() {
            Some(res) => Ok(res),
            None => Err(Future { core: Some(core) })
        }
    }

    fn ready<F: FnOnce(Future<T, E>) + Send>(mut self, f: F) -> Receipt<Future<T, E>> {
        let core = core::take(&mut self.core);

        match core.consumer_ready(move |core| f(Future::from_core(core))) {
            Some(count) => receipt::new(core, count),
            None => receipt::none(),
        }
    }

    fn await(mut self) -> AsyncResult<T, E> {
        core::take(&mut self.core).consumer_await()
    }
}

impl<T: Send, E: Send> fmt::Debug for Future<T, E> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Future {{ ... }}")
    }
}

#[unsafe_destructor]
impl<T: Send, E: Send> Drop for Future<T, E> {
    fn drop(&mut self) {
        if self.core.is_some() {
            core::take(&mut self.core).cancel();
        }
    }
}

impl<T: Send, E: Send> Cancel<Future<T, E>> for Receipt<Future<T, E>> {
    fn cancel(self) -> Option<Future<T, E>> {
        let (core, count) = receipt::parts(self);

        if !core.is_some() {
            return None;
        }

        if core::get(&core).consumer_ready_cancel(count) {
            return Some(Future { core: core });
        }

        None
    }
}

/// An object that is used to fulfill or reject an associated Future.
///
/// ```
/// use syncbox::util::async::*;
/// use syncbox::util::async::AsyncError::*;
///
/// let (tx, future) = Future::<u32, &'static str>::pair();
///
/// future.and_then(|v| {
///     assert!(v == 1);
///     Ok(v + v)
/// });
///
/// tx.complete(1);
///
/// let (tx, future) = Future::<u32, &'static str>::pair();
/// tx.fail("failed");
///
/// future.or_else(|err| {
///     match err {
///         CancellationError => unreachable!(),
///         ExecutionError(err) => assert!(err == "failed")
///     }
///
///     Ok(123)
/// });
/// ```
#[unsafe_no_drop_flag]
pub struct Complete<T: Send, E: Send> {
    core: Option<Core<T, E>>,
}

impl<T: Send, E: Send> Complete<T, E> {
    /// Fulfill the associated promise with a value
    pub fn complete(mut self, val: T) {
        core::take(&mut self.core).complete(Ok(val), true);
    }

    /// Reject the associated promise with an error. The error
    /// will be wrapped in an `ExecutionError`.
    pub fn fail(mut self, err: E) {
        core::take(&mut self.core).complete(Err(AsyncError::wrap(err)), true);
    }

    pub fn is_ready(&self) -> bool {
        core::get(&self.core).producer_is_ready()
    }

    pub fn is_err(&self) -> bool {
        core::get(&self.core).producer_is_err()
    }

    fn poll(mut self) -> Result<AsyncResult<Complete<T, E>, ()>, Complete<T, E>> {
        debug!("Complete::poll; is_ready={}", self.is_ready());

        let core = core::take(&mut self.core);

        match core.producer_poll() {
            Some(res) => Ok(res.map(Complete::from_core)),
            None => Err(Complete { core: Some(core) })
        }
    }

    pub fn ready<F: FnOnce(Complete<T, E>) + Send>(mut self, f: F) {
        core::take(&mut self.core)
            .producer_ready(move |core| f(Complete::from_core(core)));
    }

    pub fn await(self) -> AsyncResult<Complete<T, E>, ()> {
        core::get(&self.core).producer_await();
        self.poll().ok().expect("Complete not ready")
    }

    /*
     *
     * ===== Internal Helpers =====
     *
     */

    fn from_core(core: Core<T, E>) -> Complete<T, E> {
        Complete { core: Some(core) }
    }
}

impl<T: Send, E: Send> Async for Complete<T, E> {
    type Value = Complete<T, E>;
    type Error = ();
    type Cancel = Receipt<Complete<T, E>>;

    fn is_ready(&self) -> bool {
        Complete::is_ready(self)
    }

    fn is_err(&self) -> bool {
        Complete::is_err(self)
    }

    fn poll(self) -> Result<AsyncResult<Complete<T, E>, ()>, Complete<T, E>> {
        Complete::poll(self)
    }

    fn ready<F: FnOnce(Complete<T, E>) + Send>(self, f: F) -> Receipt<Complete<T, E>> {
        Complete::ready(self, f);
        receipt::none()
    }
}

#[unsafe_destructor]
impl<T: Send, E: Send> Drop for Complete<T, E> {
    fn drop(&mut self) {
        if self.core.is_some() {
            debug!("Complete::drop -- canceling future");
            core::take(&mut self.core).complete(Err(AsyncError::canceled()), true);
        }
    }
}

impl<T: Send, E: Send> fmt::Debug for Complete<T, E> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Complete {{ ... }}")
    }
}

impl<T: Send, E: Send> Cancel<Complete<T, E>> for Receipt<Complete<T, E>> {
    fn cancel(self) -> Option<Complete<T, E>> {
        None
    }
}

#[test]
pub fn test_size_of_future() {
    use std::mem;

    assert_eq!(mem::size_of::<usize>(), mem::size_of::<Future<String, String>>());
}

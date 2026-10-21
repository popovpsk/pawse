use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use super::{handle, unblock};

struct Unpark(std::thread::Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn wait<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        std::thread::park();
    }
}

#[test]
fn unblock_is_awaited_outside_tokio() {
    let name = wait(unblock(|| {
        std::thread::current().name().map(str::to_string)
    }));
    assert_eq!(name.as_deref(), Some("net-runtime"));
}

#[test]
fn blocking_work_may_block_on_the_runtime() {
    let value = wait(unblock(|| {
        handle().block_on(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            7
        })
    }));
    assert_eq!(value, 7);
}

#[test]
fn a_panic_reaches_the_awaiter() {
    let caught = std::panic::catch_unwind(|| wait(unblock(|| panic!("boom"))));
    let payload = caught.expect_err("the panic was swallowed");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"boom"));
}

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle, Runtime};

const WORKER_THREADS: usize = 2;

static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

pub fn try_handle() -> Result<&'static Handle, String> {
    match RUNTIME.get_or_init(build) {
        Ok(runtime) => Ok(runtime.handle()),
        Err(error) => Err(error.clone()),
    }
}

pub fn handle() -> &'static Handle {
    try_handle().unwrap_or_else(|error| panic!("the network runtime did not start: {error}"))
}

pub fn unblock<T, F>(work: F) -> impl Future<Output = T> + Send + 'static
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let task = handle().spawn_blocking(work);
    async move {
        match task.await {
            Ok(value) => value,
            Err(error) => std::panic::resume_unwind(error.into_panic()),
        }
    }
}

fn build() -> Result<Runtime, String> {
    Builder::new_multi_thread()
        .worker_threads(WORKER_THREADS)
        .thread_name("net-runtime")
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;

//! The async runtime. Network and disk work runs on tokio worker threads;
//! results come back to the GTK thread through a channel.

use gtk::glib;
use std::future::Future;
use std::sync::OnceLock;

pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().expect("tokio runtime"))
}

/// Run `fut` on the runtime and hand its output to `done` on the GTK thread.
pub fn spawn<T, F, D>(fut: F, done: D)
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
    D: FnOnce(T) + 'static,
{
    let (tx, rx) = async_channel::bounded(1);
    runtime().spawn(async move {
        let _ = tx.send(fut.await).await;
    });
    glib::spawn_future_local(async move {
        if let Ok(v) = rx.recv().await {
            done(v);
        }
    });
}

/// Start a producer on the runtime that sends a stream of events; `each` sees
/// them on the GTK thread in order. Returns a handle that aborts the producer.
pub fn stream<T, F, Fut, E>(make: F, mut each: E) -> tokio::task::AbortHandle
where
    T: Send + 'static,
    F: FnOnce(async_channel::Sender<T>) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
    E: FnMut(T) + 'static,
{
    let (tx, rx) = async_channel::unbounded();
    let handle = runtime().spawn(make(tx));
    glib::spawn_future_local(async move {
        while let Ok(v) = rx.recv().await {
            each(v);
        }
    });
    handle.abort_handle()
}

/// Fire and forget.
pub fn detach<F>(fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    runtime().spawn(fut);
}

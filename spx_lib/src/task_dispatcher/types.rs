use std::pin::Pin;
use std::sync::Arc;

pub type Executor<I, O> =
    Arc<dyn Fn(I) -> Pin<Box<dyn Future<Output = O> + Send + 'static>> + Send + Sync + 'static>;

pub type Handler<T> =
    Arc<dyn Fn(T) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + Sync + 'static>;

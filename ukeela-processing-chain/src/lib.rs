use ukeela_common::{Frame, Middleware, ProcessResult};

pub mod multi;
pub mod single;

pub trait ProcessingChain<T: Middleware + Send + Sync + 'static> {
    fn new() -> Self;

    fn push(&mut self, frame: Frame) -> Option<Frame>;

    fn pop(&mut self) -> impl std::future::Future<Output = Option<ProcessResult>>;

    fn add_middleware(&mut self, middleware: T);
}

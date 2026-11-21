use crate::Frame;

/// Process a frame - returns modified frame or error
pub type ProcessResult = Result<Frame, anyhow::Error>;

pub trait Middleware: Send + Sync {
    fn name(&self) -> &'static str;

    /// Process frame synchronously (for CPU path)
    fn process(&self, frame: Frame) -> ProcessResult;

    /// Process frame asynchronously (for async pipelines)
    fn process_async(&self, frame: Frame) -> impl Future<Output = ProcessResult> + Send + Sync;
}

/// No-op middleware for passthrough
#[derive(Debug)]
pub struct NoOpMiddleware;

impl Middleware for NoOpMiddleware {
    fn name(&self) -> &'static str {
        "passthrough"
    }

    async fn process_async(&self, frame: Frame) -> ProcessResult {
        Ok(frame)
    }

    fn process(&self, frame: Frame) -> ProcessResult {
        Ok(frame)
    }
}

#[macro_export]
macro_rules! define_middleware_dispatch {
    (
        $(#[$meta:meta])*
        $vis:vis enum $EnumName:ident {
            $( $variant:ident ($inner:ty) ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis enum $EnumName {
            $( $variant($inner), )+
        }

        impl $crate::Middleware for $EnumName {
            fn name(&self) -> &'static str {
                match self {
                    $( Self::$variant(v) => <$inner as $crate::Middleware>::name(v), )+
                }
            }

            fn process(&self, frame: $crate::Frame) -> $crate::ProcessResult {
                match self {
                    $( Self::$variant(v) => <$inner as $crate::Middleware>::process(v, frame), )+
                }
            }

            async fn process_async(&self, frame: $crate::Frame) -> $crate::ProcessResult {
                match self {
                    $( Self::$variant(v) => {
                        <$inner as $crate::Middleware>::process_async(v, frame).await
                    }, )+
                }
            }
        }
    };
}

use core::fmt::Debug;
use futures::Future;
use std::time::Duration;

pub trait Subtask
where
    Self::OutputError: Debug + 'static,
{
    type OutputError;

    #[allow(async_fn_in_trait)]
    async fn task(self) -> Result<(), Self::OutputError>;
}

impl<T, E> Subtask for T
where
    T: Future<Output = Result<(), E>>,
    E: Debug + std::error::Error + 'static,
{
    type OutputError = E;
    fn task(self) -> impl Future<Output = Result<(), Self::OutputError>> {
        self
    }
}

pub struct NoSubtask;

impl Subtask for NoSubtask {
    type OutputError = ();

    async fn task(self) -> Result<(), Self::OutputError> {
        loop {
            actix_rt::time::sleep(Duration::from_secs(60 * 60 * 24)).await;
        }
    }
}

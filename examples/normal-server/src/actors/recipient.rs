#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("the receiver is closed")]
    Closed,
}

pub trait Recipient<MessageType> {
    fn send(msg: MessageType) -> impl std::future::Future<Output = Result<(), SendError>>;
}

use std::pin::Pin;

pub trait UdpActorContextHandler {}

pub struct UdpActorContext<Handler: UdpActorContextHandler> {
    // subtask: Pin<Box<dyn Future<Output = anyhow::Result<()>>>>,

}
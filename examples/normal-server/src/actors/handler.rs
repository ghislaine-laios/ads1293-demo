pub trait Handler<Action> {
    type Output;
    // TODO: use async_trait crate
    fn handle(&mut self, action: Action) -> impl std::future::Future<Output = Self::Output>;
}

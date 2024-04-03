pub trait Handler<Action> {
    type Output;
    fn handle(&mut self, action: Action) -> impl std::future::Future<Output = Self::Output>;
}

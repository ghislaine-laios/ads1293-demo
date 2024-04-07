pub trait Handler<Action> {
    type Output;

    fn handle(&mut self, action: Action) -> impl std::future::Future<Output = Self::Output>;
}

pub trait ContextHandler<Action> {
    type Context;
    type Output;

    fn handle_with_context(
        &mut self,
        context: &mut Self::Context,
        action: Action,
    ) -> impl std::future::Future<Output = Self::Output>;
}

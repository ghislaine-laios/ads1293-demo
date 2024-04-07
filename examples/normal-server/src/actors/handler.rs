pub trait Handler<Action> {
    type Output;

    #[allow(async_fn_in_trait)]
    async fn handle(&mut self, action: Action) -> Self::Output;
}

pub trait ContextHandler<Action> {
    type Context;
    type Output;

    #[allow(async_fn_in_trait)]
    async fn handle_with_context(
        &mut self,
        context: &mut Self::Context,
        action: Action,
    ) -> Self::Output;
}

#[derive(Debug)]
pub struct Started;

#[derive(Debug)]
pub struct Stopping;

#[derive(Debug)]
pub struct NoActions;


#[derive(Debug)]
pub enum ActorAction {
    Continue,
    Break
}
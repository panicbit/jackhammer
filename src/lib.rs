use anyhow::*;
use tokio::{task::JoinHandle, time::{self, *}};
use std::{
    pin::Pin,
    future::Future,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct Jackhammer {
    interval: Interval,
    actions_per_interval: u32,
    action_factory: Box<dyn ActionFactory>,
}

impl Jackhammer {
    pub fn builder() -> JackhammerBuilder {
        JackhammerBuilder::new()
    }

    async fn run(mut self) -> Result<()> {
        loop {
            self.interval.tick().await;

            for _ in 0..self.actions_per_interval {
                let action = self.action_factory.next_action();

                tokio::spawn(async move {
                    // TODO: use result outcome for metrics
                    action.await;
                });
            }
        }
    }
}

pub struct JackhammerBuilder {
    actions_per_interval: u32,
    interval: Duration,
    action_factory: Box<dyn ActionFactory>,
}

impl JackhammerBuilder {
    pub fn new() -> Self {
        Self {
            actions_per_interval: 1,
            interval: Duration::from_secs(1),
            action_factory: Box::new(|| Box::pin(async { Ok(()) })),
        }
    }

    pub fn actions_per_interval(mut self, actions_per_interval: u32) -> Self {
        self.actions_per_interval = actions_per_interval;
        self
    }

    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    pub fn action_factory<AF>(mut self, action_factory: AF) -> Self
    where
        AF: ActionFactory + Send + Sync,
    {
        self.action_factory = Box::new(action_factory);
        self
    }

    pub fn start(self) -> JackhammerHandle {
        let jackhammer = Jackhammer {
            interval: time::interval(self.interval),
            actions_per_interval: self.actions_per_interval,
            action_factory: self.action_factory,
        };

        let join_handle = tokio::spawn(jackhammer.run());

        JackhammerHandle {
            join_handle
        }
    }
}

pub trait ActionFactory: Send + 'static {
    fn next_action(&mut self) -> BoxFuture<'static, Result<()>>;
}

impl dyn ActionFactory {
    pub fn from_fn<F, Fut>(factory_fn: F) -> impl ActionFactory
    where
        F: FnMut() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        factory_fn
    }
}

impl<F, Fut> ActionFactory for F
where
    F: FnMut() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn next_action(&mut self) -> BoxFuture<'static, Result<()>> {
        Box::pin(self())
    }
}

pub struct JackhammerHandle {
    join_handle: JoinHandle<Result<()>>,
}

impl JackhammerHandle {
    pub async fn join(self) -> Result<()> {
        self.join_handle.await??;
        Ok(())
    }
}

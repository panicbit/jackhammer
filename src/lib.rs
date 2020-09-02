use anyhow::*;
use tokio::{task::JoinHandle, time::{self, *}};
use std::{
    pin::Pin,
    future::Future,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + Sync + 'a>>;
type Action = Box<dyn FnMut() -> BoxFuture<'static, ()> + Send + Sync + 'static>;

pub struct Jackhammer {
    interval: Interval,
    actions_per_interval: u32,
    action: Action,
}

impl Jackhammer {
    pub fn builder() -> JackhammerBuilder {
        JackhammerBuilder::new()
    }

    async fn run(mut self) -> Result<()> {
        loop {
            self.interval.tick().await;

            for _ in 0..self.actions_per_interval {
                let action = (self.action)();

                tokio::spawn(async move {
                    action.await;
                });
            }
        }
    }
}

pub struct JackhammerBuilder {
    actions_per_interval: u32,
    interval: Duration,
    action: Action,
}

impl JackhammerBuilder {
    pub fn new() -> Self {
        Self {
            actions_per_interval: 1,
            interval: Duration::from_secs(1),
            action: Box::new(|| Box::pin(async {})),
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

    pub fn action<F, Fut, R>(mut self, mut action: F) -> Self
    where
        F: FnMut() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = R> + Send + Sync + 'static,
    {
        self.action = Box::new(move || {
            let action = action();

            Box::pin(async move {
                action.await;
            })
        });
        self
    }

    pub fn start(self) -> JackhammerHandle {
        let jackhammer = Jackhammer {
            interval: time::interval(self.interval),
            actions_per_interval: self.actions_per_interval,
            action: self.action,
        };

        let join_handle = tokio::spawn(jackhammer.run());

        JackhammerHandle {
            join_handle
        }
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

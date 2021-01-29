use std::{
    pin::Pin,
    future::Future,
};
use anyhow::*;
use metrix::{
    TelemetryTransmitter,
    instruments::{Panel, Cockpit},
    processor::{TelemetryProcessor, AggregatesProcessors},
instruments::Meter, TransmitsTelemetryData, instruments::Histogram, TimeUnit, instruments::Gauge};
use tokio::{
    task::JoinHandle,
    time::{self, *},
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct Jackhammer {
    interval: Interval,
    actions_per_interval: u32,
    action_factory: Box<dyn ActionFactory>,
    timeout: Option<Duration>,
    metrics: Metrics,
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
                let action = timeout(self.timeout, action);
                let metrics = self.metrics.clone();

                tokio::spawn(async move {
                    let start = Instant::now();

                    match action.await {
                        Ok(Ok(_)) => metrics.observed_successful_action(start.elapsed()),
                        Ok(Err(_)) =>  metrics.observed_failed_action(start.elapsed()),
                        Err(_) => metrics.observed_timed_out_action(start.elapsed()),
                    }

                    metrics.observed_finished_action(start.elapsed());
                });

                self.metrics.observed_spawned_action();
            }
        }
    }
}

pub struct JackhammerBuilder {
    actions_per_interval: u32,
    interval: Duration,
    action_factory: Box<dyn ActionFactory>,
    metrics: Metrics,
    timeout: Option<Duration>,
}

impl JackhammerBuilder {
    pub fn new() -> Self {
        Self {
            actions_per_interval: 1,
            interval: Duration::from_secs(1),
            action_factory: Box::new(|| Box::pin(async { Ok(()) })),
            metrics: Metrics::default(),
            timeout: None,
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

    pub fn timeout(mut self, timeout: impl Into<Option<Duration>>) -> Self {
        self.timeout = timeout.into();
        self
    }

    /// Should be called at most once
    pub fn instrumentation<AP>(mut self, aggregator: &mut AP) -> Self
    where
        AP: AggregatesProcessors,
    {
        self.metrics = Metrics::new(aggregator);
        self
    }

    pub fn start(self) -> JackhammerHandle {
        let jackhammer = Jackhammer {
            interval: time::interval(self.interval),
            actions_per_interval: self.actions_per_interval,
            action_factory: self.action_factory,
            metrics: self.metrics,
            timeout: self.timeout,
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

#[derive(Clone, Default)]
struct Metrics {
    telemetry_transmitter: Option<TelemetryTransmitter<Metric>>,
}

impl Metrics {
    fn new<AP>(aggregator: &mut AP) -> Self
    where
        AP: AggregatesProcessors,
    {
        let mut cockpit = Cockpit::new("jackhammer");

        let panel = Panel::named(Metric::SuccessfulActions, "successful_actions")
            .meter(Meter::new_with_defaults("per_second"))
            .histogram(
                Histogram::new_with_defaults("latency_us")
                .display_time_unit(TimeUnit::Microseconds)
            )
            .histogram(
                Histogram::new_with_defaults("latency_ms")
                .display_time_unit(TimeUnit::Milliseconds)
            );
        cockpit.add_panel(panel);

        let panel = Panel::named(Metric::FailedActions, "failed_actions")
            .meter(Meter::new_with_defaults("per_second"))
            .histogram(
                Histogram::new_with_defaults("latency_us")
                .display_time_unit(TimeUnit::Microseconds)
            )
            .histogram(
                Histogram::new_with_defaults("latency_ms")
                .display_time_unit(TimeUnit::Milliseconds)
            );
        cockpit.add_panel(panel);

        let mut panel = Panel::named(Metric::FinishedActions, "finished_actions")
            .meter(Meter::new_with_defaults("per_second"))
            .histogram(
                Histogram::new_with_defaults("latency_us")
                .display_time_unit(TimeUnit::Microseconds)
            )
            .histogram(
                Histogram::new_with_defaults("latency_ms")
                .display_time_unit(TimeUnit::Milliseconds)
            );
        panel.set_description("Actions that succeeded, failed or timed out");
        cockpit.add_panel(panel);

        let panel = Panel::named(Metric::TimedOutActions, "timed_out_actions")
            .meter(Meter::new_with_defaults("per_second"))
            .histogram(
                Histogram::new_with_defaults("latency_us")
                .display_time_unit(TimeUnit::Microseconds)
            )
            .histogram(
                Histogram::new_with_defaults("latency_ms")
                .display_time_unit(TimeUnit::Milliseconds)
            );
        cockpit.add_panel(panel);

        let panel = Panel::named(Metric::SpawnedActions, "spawned_actions")
            .gauge(Gauge::new_with_defaults("count"));
        cockpit.add_panel(panel);

        let (tx, mut rx) = TelemetryProcessor::new_pair_without_name();
        rx.add_cockpit(cockpit);
        aggregator.add_processor(rx);

        Self { telemetry_transmitter: Some(tx) }
    }

    fn observed_successful_action(&self, duration: Duration) {
        if let Some(tx) = &self.telemetry_transmitter {
            tx.observed_one_duration_now(Metric::SuccessfulActions, duration);
        }
    }

    fn observed_failed_action(&self, duration: Duration) {
        if let Some(tx) = &self.telemetry_transmitter {
            tx.observed_one_duration_now(Metric::FailedActions, duration);
        }
    }

    fn observed_finished_action(&self, duration: Duration) {
        let now = std::time::Instant::now();

        if let Some(tx) = &self.telemetry_transmitter {
            tx.observed_duration(Metric::FinishedActions, duration, now);
            tx.observed_one_value(Metric::SpawnedActions, metrix::Decrement, now);
        }
    }

    fn observed_timed_out_action(&self, duration: Duration) {
        if let Some(tx) = &self.telemetry_transmitter {
            tx.observed_one_duration_now(Metric::TimedOutActions, duration);
        }
    }

    fn observed_spawned_action(&self) {
        if let Some(tx) = &self.telemetry_transmitter {
            tx.observed_one_value_now(Metric::SpawnedActions, metrix::Increment);
        }
    }
}

#[derive(PartialEq, Eq, Copy, Clone)]
enum Metric {
    SuccessfulActions,
    FailedActions,
    FinishedActions,
    TimedOutActions,
    SpawnedActions,
}

async fn timeout<T>(duration: Option<Duration>, future: impl Future<Output = T>) -> Result<T, time::error::Elapsed> {
    match duration {
        Some(duration) => time::timeout(duration, future).await,
        None => Ok(future.await),
    }
}

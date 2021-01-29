use std::time::Duration;
use metrix::driver::DriverBuilder;
use jackhammer::Jackhammer;
use tokio::time::*;
use anyhow::*;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Opt {
    /// Number of requests per second to send
    #[structopt(short = "r", long = "rps")]
    target_requests_per_second: u32,
    /// Duration in milliseconds after which an a request times out
    #[structopt(short = "t", long = "timeout")]
    timeout: Option<u64>,
    // /// Data to send as request body. This implies a POST request.
    // #[structopt(short = "d", long = "data")]
    // data: String,
    url: reqwest::Url,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    let target_requests_per_second = opt.target_requests_per_second;
    let interval = Duration::from_secs(1) / target_requests_per_second;
    let client = reqwest::Client::new();

    let mut metrix_driver = DriverBuilder::new("webstress")
        .set_driver_metrics(false)
        .build();

    Jackhammer::builder()
        .interval(interval)
        .actions_per_interval(1)
        .instrumentation(&mut metrix_driver)
        .timeout(opt.timeout.map(Duration::from_millis))
        .action_factory(move || {
            let request = client.get(opt.url.clone()).send();

            async move {
                let mut response = request.await?
                    .error_for_status()?;

                // Consume and discard entire body.
                while response.chunk().await?.is_some() {}

                Ok(())
            }
        })
        .start();

    loop {
        let snapshot = metrix_driver.snapshot(true)?;

        print!("{}{}", termion::clear::All, termion::cursor::Goto(1, 1));

        println!("Target request rate: {:?}", target_requests_per_second);

        let mean_latency = snapshot.find("webstress/jackhammer/finished_actions/latency_ms/mean");
        println!("mean latency: {}", mean_latency);

        let success_rate = snapshot.find("webstress/jackhammer/successful_actions/per_second/one_minute/rate");
        println!("success rate: {}", success_rate);

        let error_rate = snapshot.find("webstress/jackhammer/failed_actions/per_second/one_minute/rate");
        println!("error rate: {}", error_rate);

        let timout_rate = snapshot.find("webstress/jackhammer/timed_out_actions/per_second/one_minute/rate");
        println!("timout rate: {}", timout_rate);

        let spawned_actions = snapshot.find("webstress/jackhammer/spawned_actions/count");
        println!("spawned actions: {}", spawned_actions);

        // let snapshot = snapshot.to_json(&metrix::snapshot::JsonConfig {
        //     pretty: Some(2),
        //     .. <_>::default()
        // });

        // println!("{}", snapshot);

        sleep(Duration::from_secs(1)).await;
    }
}

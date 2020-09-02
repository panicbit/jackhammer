use jackhammer::Jackhammer;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let mut n = 1;
    let action = move || {
        println!("Action #{}", n);
        n += 1;

        async {}
    };

    Jackhammer::builder()
    .actions_per_interval(100)
    .interval(Duration::from_secs(1) / 2)
    .action(action)
    .start()
    .join()
    .await
    .unwrap();
}

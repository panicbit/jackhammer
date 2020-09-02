use jackhammer::Jackhammer;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let mut n = 1;
    let action_factory = move || {
        println!("Action #{}", n);
        n += 1;

        async {
            Ok(())
        }
    };

    Jackhammer::builder()
    .actions_per_interval(100)
    .interval(Duration::from_secs(1) / 2)
    .action_factory(action_factory)
    .start()
    .join()
    .await
    .unwrap();
}

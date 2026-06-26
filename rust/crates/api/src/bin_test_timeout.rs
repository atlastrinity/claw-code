use reqwest::Client;
use std::time::Duration;

fn main() {
    let _client = Client::builder()
        .read_timeout(Duration::from_secs(30))
        .build();
}

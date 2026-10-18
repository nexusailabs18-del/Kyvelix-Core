use std::time::Duration;
use tokio::time::sleep;
use std::future::Future;

pub struct RetryPolicy {
    max_retries: u32,
    base_delay: Duration,
}

impl RetryPolicy {
    pub fn new(max_retries: u32) -> Self {
        Self { max_retries, base_delay: Duration::from_millis(100) }
    }

    pub async fn execute<F, Fut, T, E>(&self, mut op: F) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let mut attempt = 0;
        loop {
            match op().await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    attempt += 1;
                    if attempt >= self.max_retries { return Err(e); }
                    sleep(self.base_delay * 2u32.pow(attempt - 1)).await;
                }
            }
        }
    }
}

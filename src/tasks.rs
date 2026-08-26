use embassy_time::Timer;

use crate::log_info;

#[embassy_executor::task(pool_size = 1)]
pub async fn heartbeat() {
    loop {
        log_info!("heartbeat()");
        Timer::after_secs(1).await;
    }
}

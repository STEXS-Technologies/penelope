use async_trait::async_trait;
use penelope_domain::LogicalTime;
use penelope_ports::*;
use penelope_sqlite::SqliteLeaseClock;
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
pub type Error = Box<dyn std::error::Error>;
pub struct Authority;
#[async_trait]
impl ProcessAuthorizer for Authority {
    async fn authorize(
        &self,
        _request: &ProcessAuthorizationRequest,
    ) -> Result<ProcessAuthorizationDecision, PortError> {
        Ok(ProcessAuthorizationDecision::Authorized)
    }
}
pub struct Clock;
impl SqliteLeaseClock for Clock {
    fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
    }
}
#[async_trait]
impl penelope_ports::Clock for Clock {
    async fn now(&self) -> Result<LogicalTime, PortError> {
        SqliteLeaseClock::now(self)
    }
}
struct Notify(std::thread::Thread);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
pub fn run<T>(future: impl Future<Output = Result<T, PortError>>) -> Result<T, Error> {
    let waker = Waker::from(Arc::new(Notify(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(30))
        .ok_or("deadline overflow")?;
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            return Ok(result?);
        }
        if Instant::now() >= deadline {
            return Err("timer workload operation timed out".into());
        }
        std::thread::park_timeout(Duration::from_millis(1));
    }
}

//! Immediate injected ports: public service validation, without storage latency.
use async_trait::async_trait;
use penelope_domain::*;
use penelope_executor::services::{TimerDrainReport, claim_outbox, drain_timers};
use penelope_ports::{payloads::ProcessPayloadStore, *};
use std::{
    future::Future,
    num::{NonZeroU16, NonZeroU64},
    sync::Mutex,
    task::{Context, Poll, Waker},
};
pub type Error = Box<dyn std::error::Error>;
pub fn run<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
pub struct Ports {
    pub timers: Mutex<Vec<TimerLease>>,
    pub outbox: Mutex<Vec<OutboxLease>>,
    pub payloads: Mutex<Vec<ContentDigest>>,
    pub inputs: Mutex<Vec<ProcessInput>>,
    pub acknowledgements: Mutex<Vec<OutboxLeaseToken>>,
}
#[async_trait]
impl ProcessAuthorizer for Ports {
    async fn authorize(
        &self,
        _: &ProcessAuthorizationRequest,
    ) -> Result<ProcessAuthorizationDecision, PortError> {
        Ok(ProcessAuthorizationDecision::Authorized)
    }
}
#[async_trait]
impl Clock for Ports {
    async fn now(&self) -> Result<LogicalTime, PortError> {
        Ok(LogicalTime(100))
    }
}
#[async_trait]
impl TimerClaimStore for Ports {
    async fn claim_due(&self, _: &TimerClaimRequest) -> Result<Vec<TimerLease>, PortError> {
        Ok(std::mem::take(
            &mut *self.timers.lock().map_err(|_poison| PortError::Invariant)?,
        ))
    }
    async fn acknowledge(&self, lease: &TimerLease, _: LogicalTime) -> Result<(), PortError> {
        self.acknowledgements
            .lock()
            .map_err(|_poison| PortError::Invariant)?
            .push(lease.token);
        Ok(())
    }
}
#[async_trait]
impl OutboxStore for Ports {
    async fn claim(&self, _: &OutboxClaimRequest) -> Result<Vec<OutboxLease>, PortError> {
        Ok(std::mem::take(
            &mut *self.outbox.lock().map_err(|_poison| PortError::Invariant)?,
        ))
    }
    async fn acknowledge(&self, _: &OutboxLease, _: LogicalTime) -> Result<(), PortError> {
        Err(PortError::Invariant)
    }
    async fn renew(
        &self,
        _: &OutboxLease,
        _: LogicalTime,
        _: LogicalTime,
    ) -> Result<OutboxLease, PortError> {
        Err(PortError::Invariant)
    }
}
#[async_trait]
impl ProcessPayloadStore for Ports {
    async fn put_payload(
        &self,
        _: &ProcessScope,
        digest: ContentDigest,
        _: &[u8],
    ) -> Result<ContentDigest, PortError> {
        self.payloads
            .lock()
            .map_err(|_poison| PortError::Invariant)?
            .push(digest);
        Ok(digest)
    }
    async fn read_payload(&self, _: &ProcessScope, _: ContentDigest) -> Result<Vec<u8>, PortError> {
        Err(PortError::Invariant)
    }
}
#[async_trait]
impl Inbox for Ports {
    async fn accept(&self, input: &ProcessInput) -> Result<InboxAcceptanceReceipt, PortError> {
        self.inputs
            .lock()
            .map_err(|_poison| PortError::Invariant)?
            .push(input.clone());
        Ok(InboxAcceptanceReceipt::new(
            input.tenant_id.clone(),
            input.process_id.clone(),
            input.input_id.clone(),
            false,
        ))
    }
}
#[derive(Clone)]
pub struct Fixture {
    pub timers: Vec<TimerLease>,
    pub outbox: Vec<OutboxLease>,
    pub timer_request: TimerClaimRequest,
    pub outbox_request: OutboxClaimRequest,
}
impl Fixture {
    pub fn new(width: usize, batch: usize, order: u8) -> Result<Self, Error> {
        let suffix = "x".repeat(width.checked_sub(4).ok_or("short scope")?);
        let scope = ProcessScope::new(
            format!("tnt_{suffix}").parse()?,
            format!("prc_{suffix}").parse()?,
            format!("def_{suffix}").parse()?,
            format!("dfv_{suffix}").parse()?,
            ContentDigest([3; 32]),
        );
        let owner: PrincipalId = "pri_worker".parse()?;
        let mut timers = Vec::new();
        let mut outbox = Vec::new();
        for index in 0..batch {
            let action_id = format!(
                "act_{}{:04}",
                "x".repeat(width.checked_sub(8).ok_or("short action")?),
                index
            )
            .parse()?;
            let action = ProcessAction::new(
                scope.clone(),
                action_id,
                "stp_work".parse()?,
                0,
                ProcessActionKind::Timer,
                ContentDigest([4; 32]),
            );
            let token = OutboxLeaseToken::new(
                NonZeroU64::new(
                    u64::try_from(index)?
                        .checked_add(1)
                        .ok_or("token overflow")?,
                )
                .ok_or("zero token")?,
            );
            timers.push(TimerLease {
                timer: TimerSchedule {
                    action: action.clone(),
                    due_at: LogicalTime(100),
                },
                owner: owner.clone(),
                token,
                lease_expires_at: LogicalTime(200),
            });
            let mut action = action;
            action.kind = ProcessActionKind::ExternalEffect;
            outbox.push(OutboxLease {
                record: OutboxRecord::new(action),
                owner: owner.clone(),
                token,
                lease_expires_at: LogicalTime(200),
            });
        }
        if order == 1 {
            timers.reverse();
            outbox.reverse();
        }
        if order == 2 {
            let mut seed = 0x5eed_u64;
            for index in (1..timers.len()).rev() {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let bound = u64::try_from(index.checked_add(1).ok_or("shuffle bound")?)?;
                let selected = usize::try_from(seed.checked_rem(bound).ok_or("shuffle modulus")?)?;
                timers.swap(index, selected);
                outbox.swap(index, selected);
            }
        }
        if order > 2 {
            return Err("unknown order".into());
        }
        Ok(Self {
            timers,
            outbox,
            timer_request: TimerClaimRequest::new(
                scope.clone(),
                owner.clone(),
                LogicalTime(100),
                NonZeroU16::new(128).ok_or("limit")?,
            )?,
            outbox_request: OutboxClaimRequest::new(
                scope,
                owner,
                NonZeroU16::new(128).ok_or("limit")?,
            )?,
        })
    }
    pub fn prepare(&self, timers: bool) -> Ports {
        let batch = self.timers.len();
        Ports {
            timers: Mutex::new(if timers {
                self.timers.clone()
            } else {
                Vec::new()
            }),
            outbox: Mutex::new(if timers {
                Vec::new()
            } else {
                self.outbox.clone()
            }),
            payloads: Mutex::new(Vec::with_capacity(batch)),
            inputs: Mutex::new(Vec::with_capacity(batch)),
            acknowledgements: Mutex::new(Vec::with_capacity(batch)),
        }
    }
    // Keep service-specific async polling frames separate in mixed-path probes.
    #[inline(never)]
    pub fn invoke_timers(&self, ports: &Ports) -> Result<Output, Error> {
        Ok(Output::Timers(run(drain_timers(
            ports,
            ports,
            ports,
            &self.timer_request,
            ports,
        ))?))
    }
    #[inline(never)]
    pub fn invoke_outbox(&self, ports: &Ports) -> Result<Output, Error> {
        Ok(Output::Outbox(run(claim_outbox(
            ports,
            ports,
            &self.outbox_request,
            LogicalTime(100),
        ))?))
    }
    pub fn invoke(&self, ports: &Ports, timers: bool) -> Result<Output, Error> {
        if timers {
            self.invoke_timers(ports)
        } else {
            self.invoke_outbox(ports)
        }
    }
    pub fn verify(&self, ports: &Ports, output: &Output) -> Result<ContentDigest, Error> {
        let bytes = match output {
            Output::Outbox(leases) => {
                if leases != &self.outbox {
                    return Err("outbox changed".into());
                }
                serde_json::to_vec(leases)?
            }
            Output::Timers(report) => {
                let count = u16::try_from(self.timers.len())?;
                if report.claimed != count
                    || report.acknowledged != count
                    || report.duplicate_inputs != 0
                {
                    return Err("timer report".into());
                }
                let inputs = ports.inputs.lock().map_err(|_poison| "input lock")?;
                let payloads = ports.payloads.lock().map_err(|_poison| "payload lock")?;
                let acknowledgements = ports
                    .acknowledgements
                    .lock()
                    .map_err(|_poison| "ack lock")?;
                if inputs.len() != self.timers.len()
                    || payloads.len() != inputs.len()
                    || acknowledgements.len() != inputs.len()
                {
                    return Err("delivery count".into());
                }
                for ((input, digest), (token, timer)) in inputs
                    .iter()
                    .zip(payloads.iter())
                    .zip(acknowledgements.iter().zip(&self.timers))
                {
                    if input.kind != ProcessInputKind::TimerFired
                        || input.payload_digest != *digest
                        || token != &timer.token
                        || input.tenant_id != timer.timer.action.tenant_id
                        || input.process_id != timer.timer.action.process_id
                    {
                        return Err("delivery mismatch".into());
                    }
                }
                serde_json::to_vec(&*inputs)?
            }
        };
        Ok(ContentDigest::sha256(&bytes))
    }
}
pub enum Output {
    Timers(TimerDrainReport),
    Outbox(Vec<OutboxLease>),
}

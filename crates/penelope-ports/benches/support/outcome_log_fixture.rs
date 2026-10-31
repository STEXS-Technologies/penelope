//! Shared typed history; construction and exact verification are outside timing.
use penelope_domain::*;
use penelope_ports::OutcomeLog;
pub(super) type Error = Box<dyn std::error::Error>;

pub(super) struct Fixture {
    pub scope: ProcessScope,
    pub outcomes: Vec<ProcessOutcome>,
    pub checksum: ContentDigest,
}
impl Fixture {
    pub fn new(scope_len: usize, id_len: usize, count: usize) -> Result<Self, Error> {
        let suffix = "x".repeat(scope_len.checked_sub(4).ok_or("short scope")?);
        let scope = ProcessScope::new(
            format!("tnt_{suffix}").parse()?,
            format!("prc_{suffix}").parse()?,
            format!("def_{suffix}").parse()?,
            format!("dfv_{suffix}").parse()?,
            ContentDigest([9; 32]),
        );
        let padding = "x".repeat(id_len.checked_sub(8).ok_or("short outcome ID")?);
        let mut outcomes = Vec::with_capacity(count);
        for sequence in 0..count {
            outcomes.push(ProcessOutcome::new(
                scope.clone(),
                u64::try_from(sequence)?,
                ProcessOutcomeFact::new(
                    format!("out_{padding}{sequence:04}").parse()?,
                    CausationId::Input("inp_bench".parse()?),
                    OutcomeActor::System,
                    LogicalTime(u64::try_from(sequence)?),
                    if sequence == 0 {
                        ProcessOutcomeKind::Started
                    } else {
                        ProcessOutcomeKind::ActionPlanned
                    },
                    ContentDigest([2; 32]),
                ),
            ));
        }
        let checksum = ContentDigest::sha256(&serde_json::to_vec(&outcomes)?);
        Ok(Self {
            scope,
            outcomes,
            checksum,
        })
    }
    pub fn append(&self, log: &mut OutcomeLog, chunk: usize) -> Result<(), Error> {
        for batch in self.outcomes.chunks(chunk) {
            log.append(batch)?;
        }
        Ok(())
    }
    pub fn verify(&self, log: &OutcomeLog) -> Result<(), Error> {
        if log.scope() != &self.scope
            || log.outcomes() != self.outcomes
            || log.next_sequence() != u64::try_from(self.outcomes.len())?
        {
            return Err("incorrect outcome log".into());
        }
        Ok(())
    }
}
pub(super) const PROFILES: [(&str, usize, usize); 4] = [
    ("one", 1, 1),
    ("page", 128, 128),
    ("stream", 4096, 1),
    ("rebuild", 4096, 4096),
];

use std::time::{Duration, Instant};
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(60);
#[derive(Default)]
pub struct SessionPolicy {
    generation: u64,
    last_interaction: Option<Instant>,
}
impl SessionPolicy {
    pub fn token(&self) -> u64 {
        self.generation
    }
    pub fn accepts(&self, token: u64) -> bool {
        self.generation == token
    }
    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn touch(&mut self, now: Instant) {
        self.last_interaction = Some(now);
    }
    pub fn expired(&self, now: Instant) -> bool {
        self.last_interaction
            .is_some_and(|last| now.saturating_duration_since(last) >= IDLE_TIMEOUT)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activity_deadline_and_stale_unlock_are_independent() {
        let mut policy = SessionPolicy::default();
        let now = Instant::now();
        let old = policy.token();
        policy.touch(now);
        assert!(!policy.expired(now + IDLE_TIMEOUT - Duration::from_millis(1)));
        assert!(policy.expired(now + IDLE_TIMEOUT));
        policy.touch(now + IDLE_TIMEOUT);
        assert!(!policy.expired(now + IDLE_TIMEOUT));
        policy.invalidate();
        assert!(!policy.accepts(old));
    }
}

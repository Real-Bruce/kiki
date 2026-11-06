/// A generation belongs to one capture request and can never be reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Capturing(SessionId),
    Editing,
}

pub struct SessionGate {
    next: u64,
    phase: Phase,
}
impl Default for SessionGate {
    fn default() -> Self {
        Self {
            next: 0,
            phase: Phase::Idle,
        }
    }
}
impl SessionGate {
    pub fn start(&mut self) -> Result<Option<SessionId>, &'static str> {
        if self.phase != Phase::Idle {
            return Ok(None);
        }
        let next = self
            .next
            .checked_add(1)
            .ok_or("截图会话计数已耗尽，请重新启动应用")?;
        self.next = next;
        let id = SessionId(next);
        self.phase = Phase::Capturing(id);
        Ok(Some(id))
    }
    pub fn accept_frame(&mut self, id: SessionId) -> bool {
        if self.phase != Phase::Capturing(id) {
            return false;
        }
        self.phase = Phase::Editing;
        true
    }
    pub fn cancel(&mut self) {
        self.phase = Phase::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_discards_late_frames_without_mutating_new_session() {
        let mut gate = SessionGate::default();
        let first = gate.start().unwrap().unwrap();
        assert_eq!(gate.start().unwrap(), None);
        gate.cancel();
        let second = gate.start().unwrap().unwrap();
        assert_ne!(first, second);
        assert!(!gate.accept_frame(first));
        assert!(gate.accept_frame(second));
        assert!(!gate.accept_frame(second));
        assert_eq!(gate.start().unwrap(), None);
        gate.cancel();
        assert!(gate.start().unwrap().is_some());
    }
    #[test]
    fn generation_exhaustion_is_recoverable() {
        let mut gate = SessionGate {
            next: u64::MAX,
            phase: Phase::Idle,
        };
        assert!(gate.start().is_err());
        assert_eq!(gate.phase, Phase::Idle);
    }
}

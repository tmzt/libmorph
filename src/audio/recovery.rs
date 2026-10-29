//! When to reopen a lost stream, and what to tell the app (plan
//! `20261029_02_AUDIO_STREAM_RECOVERY_PLAN.md`, section 3). No cpal, AAudio
//! or clock of its own: the owner passes the time and the stream's callback
//! counter, and does what [`Act`] says.

use std::time::Duration;

use web_time::Instant;

use super::{AudioEvent, AudioState, AudioStream};

const fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

/// Delays before attempt 1, 2, 3 and 4 after a loss. Then `Failed`.
pub(crate) const RETRY_DELAYS: [Duration; 4] = [ms(0), ms(250), ms(1000), ms(3000)];
/// A stream that should run and has delivered no block (capture) or called
/// no callback (output) for this long is lost. The same limit applies from
/// each (re)start to its first block.
pub(crate) const STALL: Duration = ms(1000);

/// What the owner of the stream must do now.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Act {
    Nothing,
    /// Close the stream and open a new one with the same configuration,
    /// then call [`Recovery::reopened`] with the result.
    Reopen,
}

pub(crate) struct Recovery {
    stream: AudioStream,
    state: AudioState,
    /// Attempts made since the loss (0 while `Running`).
    attempts: u32,
    /// When the next attempt is due, while `Recovering` between attempts.
    retry_at: Option<Instant>,
    /// While `Recovering`: a reopen succeeded and its first block is
    /// awaited (from `seen_at`).
    waiting: bool,
    /// The callback counter last seen, and when it last changed (or when
    /// the stream was last started).
    seen: u64,
    seen_at: Instant,
    /// When the stream was lost, for the `restored` line.
    lost_at: Instant,
    /// The last error, for `Failed`.
    reason: String,
    events: Vec<AudioEvent>,
}

impl Recovery {
    /// The stream opened and started (or opened paused: [`paused`](Self::paused) next).
    pub fn opened(stream: AudioStream, now: Instant, counter: u64) -> Self {
        Self {
            stream,
            state: AudioState::Running,
            attempts: 0,
            retry_at: None,
            waiting: false,
            seen: counter,
            seen_at: now,
            lost_at: now,
            reason: String::new(),
            events: Vec::new(),
        }
    }

    /// The app paused: no watching, no retries. Keeps `Failed`.
    pub fn paused(&mut self) {
        if self.state != AudioState::Failed {
            self.state = AudioState::Paused;
            self.attempts = 0;
            self.retry_at = None;
            self.waiting = false;
        }
    }

    /// The app resumed and the stream's start returned `result`. Ok:
    /// `Running`, watched from `now` (from `Failed`, its first block is
    /// `Restored`). Err: a loss (`Interrupted`) and `Act::Reopen` at once.
    /// From `Recovering` or `Failed`, the attempts start afresh.
    pub fn resumed(&mut self, now: Instant, counter: u64, result: Result<(), String>) -> Act {
        let was_failed = self.state == AudioState::Failed;
        self.attempts = 0;
        self.retry_at = None;
        self.waiting = false;
        self.seen = counter;
        self.seen_at = now;
        match result {
            Ok(()) if was_failed => {
                // Started again with no attempt: the first block restores it.
                self.state = AudioState::Recovering;
                self.waiting = true;
                Act::Nothing
            }
            Ok(()) => {
                self.state = AudioState::Running;
                Act::Nothing
            }
            Err(error) => {
                self.state = AudioState::Running;
                log::warn!(
                    "libmorph: {} resume failed: {error}; reopening",
                    self.name()
                );
                self.loss(now, error)
            }
        }
    }

    /// The stream's error callback reported a device loss.
    #[allow(dead_code)] // Until the error callbacks report losses (step 4).
    pub fn lost(&mut self, now: Instant, reason: String) -> Act {
        match self.state {
            AudioState::Running => {
                log::warn!("libmorph: {} lost: {reason}; reopening", self.name());
                self.loss(now, reason)
            }
            // The stream a reopen gave was lost before its first block.
            AudioState::Recovering if self.waiting => {
                self.failed_attempt(now, reason);
                self.due(now)
            }
            _ => Act::Nothing,
        }
    }

    /// Once per frame: the stall check, the first block after a reopen,
    /// and attempts that are due.
    pub fn tick(&mut self, now: Instant, counter: u64) -> Act {
        let moved = counter != self.seen;
        if moved {
            self.seen = counter;
            self.seen_at = now;
        }
        let stalled = !moved && now.duration_since(self.seen_at) >= STALL;
        match self.state {
            AudioState::Running if stalled => {
                log::warn!(
                    "libmorph: {} stalled: no data for {} ms; reopening",
                    self.name(),
                    STALL.as_millis()
                );
                self.loss(now, stall_reason())
            }
            AudioState::Recovering if self.waiting && moved => {
                self.state = AudioState::Running;
                self.waiting = false;
                log::info!(
                    "libmorph: {} restored after {} attempt(s), {} ms after the loss",
                    self.name(),
                    self.attempts,
                    now.duration_since(self.lost_at).as_millis()
                );
                self.events.push(AudioEvent::Restored {
                    stream: self.stream,
                    attempts: self.attempts,
                });
                self.attempts = 0;
                Act::Nothing
            }
            AudioState::Recovering if self.waiting && stalled => {
                self.failed_attempt(now, stall_reason());
                self.due(now)
            }
            AudioState::Recovering => self.due(now),
            _ => Act::Nothing,
        }
    }

    /// The reopen asked for by `Act::Reopen` returned `result`. Ok: wait for
    /// the first block (see [`tick`](Self::tick)). Err: the next delay, or
    /// `Failed` after the fourth attempt.
    pub fn reopened(&mut self, now: Instant, counter: u64, result: Result<(), String>) {
        if self.state != AudioState::Recovering {
            return;
        }
        match result {
            Ok(()) => {
                self.waiting = true;
                self.seen = counter;
                self.seen_at = now;
            }
            Err(error) => self.failed_attempt(now, error),
        }
    }

    /// `Audio::restart`: from `Failed`, attempts start afresh
    /// (`Act::Reopen`). Anything else: nothing to do.
    pub fn restart(&mut self, now: Instant) -> Act {
        if self.state != AudioState::Failed {
            return Act::Nothing;
        }
        log::info!("libmorph: {} restart asked", self.name());
        self.state = AudioState::Recovering;
        self.attempts = 0;
        self.waiting = false;
        self.retry_at = Some(now);
        self.lost_at = now;
        self.due(now)
    }

    pub fn state(&self) -> AudioState {
        self.state
    }

    /// Attempts made since the loss: the number of the one under way.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn take_events(&mut self) -> Vec<AudioEvent> {
        std::mem::take(&mut self.events)
    }

    /// `audio capture` or `audio output`, as the log lines name it.
    pub fn name(&self) -> &'static str {
        name(self.stream)
    }

    /// A loss while `Running`: `Recovering`, one `Interrupted`, and the
    /// first attempt (at once).
    fn loss(&mut self, now: Instant, reason: String) -> Act {
        self.state = AudioState::Recovering;
        self.attempts = 0;
        self.waiting = false;
        self.retry_at = Some(now + RETRY_DELAYS[0]);
        self.lost_at = now;
        self.reason.clone_from(&reason);
        self.events.push(AudioEvent::Interrupted {
            stream: self.stream,
            reason,
        });
        self.due(now)
    }

    /// An attempt that failed to open, or opened and gave nothing.
    fn failed_attempt(&mut self, now: Instant, reason: String) {
        self.waiting = false;
        log::warn!(
            "libmorph: {} reopen {}/{} failed: {reason}",
            self.name(),
            self.attempts,
            RETRY_DELAYS.len()
        );
        self.reason = reason;
        match RETRY_DELAYS.get(self.attempts as usize) {
            Some(delay) => self.retry_at = Some(now + *delay),
            None => {
                self.state = AudioState::Failed;
                self.retry_at = None;
                log::error!(
                    "libmorph: {} failed after {} attempts: {}; restart() or the next resume tries again",
                    self.name(),
                    RETRY_DELAYS.len(),
                    self.reason
                );
                self.events.push(AudioEvent::Failed {
                    stream: self.stream,
                    reason: self.reason.clone(),
                });
            }
        }
    }

    /// `Act::Reopen` if an attempt is due, counting it.
    fn due(&mut self, now: Instant) -> Act {
        match self.retry_at {
            Some(at) if self.state == AudioState::Recovering && !self.waiting && at <= now => {
                self.retry_at = None;
                self.attempts += 1;
                Act::Reopen
            }
            _ => Act::Nothing,
        }
    }
}

pub(crate) fn name(stream: AudioStream) -> &'static str {
    match stream {
        AudioStream::Output => "audio output",
        AudioStream::Capture => "audio capture",
    }
}

fn stall_reason() -> String {
    format!("no data for {} ms", STALL.as_millis())
}

#[cfg(test)]
mod tests {
    use super::{Act, RETRY_DELAYS, Recovery, STALL, ms};
    use crate::{AudioEvent, AudioState, AudioStream};
    use web_time::Instant;

    const CAPTURE: AudioStream = AudioStream::Capture;

    fn interrupted(reason: &str) -> AudioEvent {
        AudioEvent::Interrupted {
            stream: CAPTURE,
            reason: reason.into(),
        }
    }

    /// Paused, then resumed with `InvalidState`, as on the S23.
    fn failed_resume(t0: Instant) -> Recovery {
        let mut recovery = Recovery::opened(CAPTURE, t0, 0);
        recovery.paused();
        assert_eq!(
            recovery.resumed(t0, 0, Err("InvalidState".into())),
            Act::Reopen
        );
        recovery
    }

    #[test]
    fn failed_start_reopens_at_once_and_reports_interrupted() {
        let t0 = Instant::now();
        let mut recovery = failed_resume(t0);
        assert_eq!(recovery.state(), AudioState::Recovering);
        assert_eq!(recovery.attempts(), 1);
        assert_eq!(recovery.take_events(), [interrupted("InvalidState")]);
        assert!(recovery.take_events().is_empty(), "taken once");
    }

    #[test]
    fn first_block_after_reopen_is_restored_with_attempts() {
        let t0 = Instant::now();
        let mut recovery = failed_resume(t0);
        recovery.reopened(t0, 0, Ok(()));
        assert_eq!(recovery.tick(t0 + ms(10), 0), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Recovering);
        assert_eq!(recovery.tick(t0 + ms(20), 1), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Running);
        assert_eq!(
            recovery.take_events(),
            [
                interrupted("InvalidState"),
                AudioEvent::Restored {
                    stream: CAPTURE,
                    attempts: 1
                }
            ]
        );
        // Watched again from the block: blocks keep it running.
        assert_eq!(recovery.tick(t0 + ms(900), 2), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Running);
    }

    #[test]
    fn stall_while_running_is_a_loss() {
        let t0 = Instant::now();
        let mut recovery = Recovery::opened(CAPTURE, t0, 0);
        assert_eq!(recovery.tick(t0 + ms(500), 5), Act::Nothing);
        assert_eq!(recovery.tick(t0 + ms(1400), 5), Act::Nothing, "900 ms");
        assert_eq!(recovery.state(), AudioState::Running);
        assert_eq!(recovery.tick(t0 + ms(1500), 5), Act::Reopen, "1000 ms");
        assert_eq!(recovery.state(), AudioState::Recovering);
        assert_eq!(recovery.take_events(), [interrupted("no data for 1000 ms")]);
    }

    #[test]
    fn no_stall_and_no_retry_while_paused() {
        let t0 = Instant::now();
        let mut recovery = Recovery::opened(CAPTURE, t0, 0);
        recovery.paused();
        assert_eq!(recovery.tick(t0 + ms(60_000), 0), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Paused);
        // Paused while recovering: the attempts wait for the resume.
        let mut recovery = failed_resume(t0);
        recovery.reopened(t0, 0, Err("Disconnected".into()));
        recovery.paused();
        assert_eq!(recovery.tick(t0 + ms(60_000), 0), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Paused);
        assert_eq!(recovery.lost(t0 + ms(60_000), "gone".into()), Act::Nothing);
        // Back with a working start: running, watched from the resume.
        assert_eq!(recovery.resumed(t0 + ms(60_000), 0, Ok(())), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Running);
        assert_eq!(recovery.tick(t0 + ms(60_500), 1), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Running);
    }

    #[test]
    fn retries_follow_the_delays_then_fail_once() {
        let t0 = Instant::now();
        let mut recovery = failed_resume(t0);
        let mut now = t0;
        recovery.reopened(now, 0, Err("e1".into()));
        for (attempt, delay) in RETRY_DELAYS.iter().enumerate().skip(1) {
            let due = now + *delay;
            assert_eq!(recovery.tick(due - ms(1), 0), Act::Nothing, "early");
            assert_eq!(recovery.tick(due, 0), Act::Reopen);
            assert_eq!(recovery.attempts(), attempt as u32 + 1);
            now = due;
            recovery.reopened(now, 0, Err(format!("e{}", attempt + 1)));
        }
        assert_eq!(recovery.state(), AudioState::Failed);
        assert_eq!(recovery.tick(now + ms(60_000), 0), Act::Nothing);
        assert_eq!(
            recovery.take_events(),
            [
                interrupted("InvalidState"),
                AudioEvent::Failed {
                    stream: CAPTURE,
                    reason: "e4".into()
                }
            ]
        );
    }

    #[test]
    fn reopen_without_blocks_counts_as_failed() {
        let t0 = Instant::now();
        let mut recovery = failed_resume(t0);
        recovery.reopened(t0, 0, Ok(()));
        assert_eq!(recovery.tick(t0 + STALL - ms(1), 0), Act::Nothing);
        // No block within STALL: attempt 1 failed, attempt 2 in 250 ms.
        assert_eq!(recovery.tick(t0 + STALL, 0), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Recovering);
        assert_eq!(recovery.tick(t0 + STALL + ms(250), 0), Act::Reopen);
        assert_eq!(recovery.attempts(), 2);
        // A loss reported on the new stream before its first block fails
        // that attempt too.
        recovery.reopened(t0 + STALL + ms(250), 0, Ok(()));
        assert_eq!(
            recovery.lost(t0 + STALL + ms(300), "Disconnected".into()),
            Act::Nothing
        );
        assert_eq!(recovery.tick(t0 + STALL + ms(1300), 0), Act::Reopen);
        assert_eq!(recovery.attempts(), 3);
        assert_eq!(recovery.take_events(), [interrupted("InvalidState")]);
    }

    #[test]
    fn resume_after_failed_starts_afresh() {
        let t0 = Instant::now();
        let mut recovery = failed_resume(t0);
        for _ in 0..4 {
            recovery.reopened(t0, 0, Err("Disconnected".into()));
            let _ = recovery.tick(t0 + ms(10_000), 0);
        }
        assert_eq!(recovery.state(), AudioState::Failed);
        recovery.take_events();
        // The pause keeps Failed; the resume tries again, four times.
        recovery.paused();
        assert_eq!(recovery.state(), AudioState::Failed);
        let t1 = t0 + ms(20_000);
        assert_eq!(recovery.resumed(t1, 0, Err("closed".into())), Act::Reopen);
        assert_eq!(recovery.attempts(), 1);
        assert_eq!(recovery.take_events(), [interrupted("closed")]);
        recovery.reopened(t1, 0, Ok(()));
        assert_eq!(recovery.tick(t1 + ms(5), 3), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Running);
        // A start that works from Failed waits for its first block.
        let mut recovery = failed_resume(t0);
        for _ in 0..4 {
            recovery.reopened(t0, 0, Err("Disconnected".into()));
            let _ = recovery.tick(t0 + ms(10_000), 0);
        }
        recovery.take_events();
        assert_eq!(recovery.resumed(t1, 0, Ok(())), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Recovering);
        assert_eq!(recovery.tick(t1 + ms(5), 1), Act::Nothing);
        assert_eq!(recovery.state(), AudioState::Running);
        assert_eq!(
            recovery.take_events(),
            [AudioEvent::Restored {
                stream: CAPTURE,
                attempts: 0
            }]
        );
    }

    #[test]
    fn restart_only_from_failed() {
        let t0 = Instant::now();
        let mut recovery = Recovery::opened(CAPTURE, t0, 0);
        assert_eq!(recovery.restart(t0), Act::Nothing, "running");
        recovery.paused();
        assert_eq!(recovery.restart(t0), Act::Nothing, "paused");
        let mut recovery = failed_resume(t0);
        assert_eq!(recovery.restart(t0), Act::Nothing, "recovering");
        for _ in 0..4 {
            recovery.reopened(t0, 0, Err("Disconnected".into()));
            let _ = recovery.tick(t0 + ms(10_000), 0);
        }
        assert_eq!(recovery.state(), AudioState::Failed);
        assert_eq!(recovery.restart(t0 + ms(20_000)), Act::Reopen);
        assert_eq!(recovery.state(), AudioState::Recovering);
        assert_eq!(recovery.attempts(), 1);
    }

    #[test]
    fn second_loss_while_recovering_reports_nothing() {
        let t0 = Instant::now();
        let mut recovery = failed_resume(t0);
        recovery.reopened(t0, 0, Err("Disconnected".into()));
        assert_eq!(recovery.lost(t0 + ms(10), "again".into()), Act::Nothing);
        assert_eq!(recovery.tick(t0 + ms(5_000), 0), Act::Reopen);
        assert_eq!(recovery.take_events(), [interrupted("InvalidState")]);
    }
}

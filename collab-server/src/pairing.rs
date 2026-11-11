//! Pairing codes: add a machine to a team without copying the team token.
//!
//! The pattern is the one a TV uses to sign in. A machine already in the
//! team asks for a code and shows it. The person types that code on the new
//! machine, which trades it for a team token of its own.
//!
//! A six-digit code is short enough to guess, so the table is strict:
//!
//! * a code works once,
//! * it expires after [`CODE_TTL`],
//! * [`MAX_FAILED_ATTEMPTS`] wrong guesses cancel every outstanding code.
//!
//! With at most a few codes alive and five guesses before they are all
//! cancelled, a guesser's odds are a few in a million per attempt window.
//! That is fine for machines you control on a network you trust. It is not
//! a substitute for the team token on a hostile network, where the token
//! itself would be exposed anyway unless the server is behind TLS.
//!
//! Codes live in memory only. Restarting the server cancels them, which is
//! the safe direction.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long a code can be redeemed after it is issued.
pub const CODE_TTL: Duration = Duration::from_secs(300);

/// Wrong guesses allowed before every outstanding code is cancelled.
pub const MAX_FAILED_ATTEMPTS: u32 = 5;

/// Most codes kept at once. Asking for more drops the oldest.
pub const MAX_ACTIVE_CODES: usize = 32;

/// Digits in a code.
pub const CODE_LEN: usize = 6;

/// Why a code was not accepted. One variant on purpose: the caller must not
/// be able to tell "never existed" from "expired" from "already used".
#[derive(Debug, PartialEq, Eq)]
pub struct InvalidCode;

struct Pending {
    team_id: String,
    issued_at: Instant,
    /// Order of issue. Two codes can share an `issued_at`, so age alone
    /// can't say which is older.
    order: u64,
}

/// Outstanding pairing codes for every team on this server.
#[derive(Default)]
pub struct PairingTable {
    codes: HashMap<String, Pending>,
    failed_attempts: u32,
    issued_so_far: u64,
}

impl PairingTable {
    /// Create a code that lets one machine join `team_id`.
    ///
    /// `random` supplies uniformly random numbers; the caller passes a real
    /// source in production and a fixed one in tests.
    pub fn issue(
        &mut self,
        team_id: &str,
        now: Instant,
        mut random: impl FnMut() -> u32,
    ) -> String {
        self.drop_expired(now);

        // Keep the table bounded: the oldest code makes room.
        while self.codes.len() >= MAX_ACTIVE_CODES {
            let oldest = self
                .codes
                .iter()
                .min_by_key(|(_, p)| p.order)
                .map(|(code, _)| code.clone());
            match oldest {
                Some(code) => self.codes.remove(&code),
                None => break,
            };
        }

        let code = loop {
            let candidate = Self::to_code(&mut random);
            if !self.codes.contains_key(&candidate) {
                break candidate;
            }
        };
        self.issued_so_far += 1;
        self.codes.insert(
            code.clone(),
            Pending {
                team_id: team_id.to_string(),
                issued_at: now,
                order: self.issued_so_far,
            },
        );
        code
    }

    /// Trade a code for the team it was issued for. Consumes the code.
    pub fn redeem(&mut self, code: &str, now: Instant) -> Result<String, InvalidCode> {
        self.drop_expired(now);

        let well_formed = code.len() == CODE_LEN && code.bytes().all(|b| b.is_ascii_digit());
        if well_formed && let Some(pending) = self.codes.remove(code) {
            return Ok(pending.team_id);
        }

        // A wrong guess. Enough of them and every outstanding code goes,
        // so nobody can walk the six-digit space. The count is only reset
        // here, never by a successful pairing.
        self.failed_attempts += 1;
        if self.failed_attempts >= MAX_FAILED_ATTEMPTS {
            self.codes.clear();
            self.failed_attempts = 0;
        }
        Err(InvalidCode)
    }

    /// Codes that could still be redeemed at `now`.
    pub fn active(&self, now: Instant) -> usize {
        self.codes
            .values()
            .filter(|p| !Self::expired(p, now))
            .count()
    }

    fn expired(pending: &Pending, now: Instant) -> bool {
        now.saturating_duration_since(pending.issued_at) >= CODE_TTL
    }

    fn drop_expired(&mut self, now: Instant) {
        self.codes.retain(|_, p| !Self::expired(p, now));
    }

    /// A uniformly random code. Plain `n % 1_000_000` would favour the low
    /// codes slightly, because 2^32 is not a multiple of a million; numbers
    /// from the uneven top slice are thrown away instead.
    fn to_code(random: &mut impl FnMut() -> u32) -> String {
        const SPACE: u32 = 1_000_000;
        const LIMIT: u32 = u32::MAX - (u32::MAX % SPACE);
        loop {
            let n = random();
            if n < LIMIT {
                return format!("{:0width$}", n % SPACE, width = CODE_LEN);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A random source that hands out the given numbers in order.
    fn seq(values: &[u32]) -> impl FnMut() -> u32 + '_ {
        let mut i = 0;
        move || {
            let v = values[i % values.len()];
            i += 1;
            v
        }
    }

    #[test]
    fn a_code_is_six_digits_and_names_its_team() {
        let mut t = PairingTable::default();
        let now = Instant::now();
        let code = t.issue("team-a", now, seq(&[123_456]));

        assert_eq!(code.len(), CODE_LEN);
        assert!(code.chars().all(|c| c.is_ascii_digit()), "{code}");
        assert_eq!(t.redeem(&code, now), Ok("team-a".to_string()));
    }

    #[test]
    fn small_numbers_keep_their_leading_zeros() {
        let mut t = PairingTable::default();
        let code = t.issue("team-a", Instant::now(), seq(&[42]));
        assert_eq!(code, "000042");
    }

    #[test]
    fn a_code_works_only_once() {
        let mut t = PairingTable::default();
        let now = Instant::now();
        let code = t.issue("team-a", now, seq(&[111_111]));

        assert!(t.redeem(&code, now).is_ok());
        assert_eq!(t.redeem(&code, now), Err(InvalidCode));
    }

    #[test]
    fn a_code_expires() {
        let mut t = PairingTable::default();
        let issued = Instant::now();
        let code = t.issue("team-a", issued, seq(&[222_222]));

        let just_inside = issued + CODE_TTL - Duration::from_secs(1);
        assert_eq!(t.active(just_inside), 1);

        let just_after = issued + CODE_TTL + Duration::from_secs(1);
        assert_eq!(t.active(just_after), 0);
        assert_eq!(t.redeem(&code, just_after), Err(InvalidCode));
    }

    #[test]
    fn each_code_belongs_to_the_team_that_asked() {
        let mut t = PairingTable::default();
        let now = Instant::now();
        let a = t.issue("team-a", now, seq(&[100_001]));
        let b = t.issue("team-b", now, seq(&[100_002]));

        assert_eq!(t.redeem(&b, now), Ok("team-b".to_string()));
        assert_eq!(t.redeem(&a, now), Ok("team-a".to_string()));
    }

    #[test]
    fn a_collision_with_a_live_code_draws_again() {
        let mut t = PairingTable::default();
        let now = Instant::now();
        let first = t.issue("team-a", now, seq(&[555_555]));
        // The source repeats 555555 once before offering something new.
        let second = t.issue("team-b", now, seq(&[555_555, 666_666]));

        assert_eq!(first, "555555");
        assert_eq!(second, "666666");
        assert_eq!(t.redeem(&first, now), Ok("team-a".to_string()));
    }

    #[test]
    fn a_few_wrong_guesses_do_no_harm() {
        let mut t = PairingTable::default();
        let now = Instant::now();
        let code = t.issue("team-a", now, seq(&[777_777]));

        for _ in 0..MAX_FAILED_ATTEMPTS - 1 {
            assert_eq!(t.redeem("000000", now), Err(InvalidCode));
        }
        assert_eq!(t.redeem(&code, now), Ok("team-a".to_string()));
    }

    #[test]
    fn too_many_wrong_guesses_cancel_every_outstanding_code() {
        let mut t = PairingTable::default();
        let now = Instant::now();
        let a = t.issue("team-a", now, seq(&[777_777]));
        let b = t.issue("team-b", now, seq(&[888_888]));

        for _ in 0..MAX_FAILED_ATTEMPTS {
            assert_eq!(t.redeem("000000", now), Err(InvalidCode));
        }
        assert_eq!(t.active(now), 0);
        assert_eq!(t.redeem(&a, now), Err(InvalidCode));
        assert_eq!(t.redeem(&b, now), Err(InvalidCode));
    }

    #[test]
    fn pairing_works_again_after_a_cancellation() {
        let mut t = PairingTable::default();
        let now = Instant::now();
        t.issue("team-a", now, seq(&[777_777]));
        for _ in 0..MAX_FAILED_ATTEMPTS {
            let _ = t.redeem("000000", now);
        }

        let fresh = t.issue("team-a", now, seq(&[999_999]));
        assert_eq!(t.redeem(&fresh, now), Ok("team-a".to_string()));
    }

    #[test]
    fn a_successful_pairing_does_not_forgive_earlier_wrong_guesses() {
        // Otherwise someone holding one valid code could reset the counter
        // forever while guessing at the others.
        let mut t = PairingTable::default();
        let now = Instant::now();
        let mine = t.issue("team-a", now, seq(&[100_001]));
        let theirs = t.issue("team-b", now, seq(&[100_002]));

        for _ in 0..MAX_FAILED_ATTEMPTS - 1 {
            let _ = t.redeem("000000", now);
        }
        assert!(t.redeem(&mine, now).is_ok());
        assert_eq!(t.redeem("000000", now), Err(InvalidCode));
        assert_eq!(t.redeem(&theirs, now), Err(InvalidCode));
    }

    #[test]
    fn junk_is_rejected_and_counts_as_a_guess() {
        let mut t = PairingTable::default();
        let now = Instant::now();
        let code = t.issue("team-a", now, seq(&[100_001]));

        for junk in ["", "12345", "1234567", "12a456", "１２３４５６"] {
            assert_eq!(t.redeem(junk, now), Err(InvalidCode), "{junk:?}");
        }
        // Five guesses were spent on junk, so the real code is gone too.
        assert_eq!(t.redeem(&code, now), Err(InvalidCode));
    }

    #[test]
    fn the_table_never_grows_past_its_cap() {
        let mut t = PairingTable::default();
        let now = Instant::now();
        let mut n = 0u32;
        let first = t.issue("team-a", now, || {
            n += 1;
            n
        });
        for _ in 0..MAX_ACTIVE_CODES {
            t.issue("team-a", now, || {
                n += 1;
                n
            });
        }

        assert_eq!(t.active(now), MAX_ACTIVE_CODES);
        // The oldest made room for the newest.
        assert_eq!(t.redeem(&first, now), Err(InvalidCode));
    }
}

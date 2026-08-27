//! Doc 17 section 2.4's mastery rule, as arithmetic and nothing else.
//!
//! One score per concept, moved by evidence: `mastery' = mastery + k(outcome −
//! mastery)`. Everything here is a pure function of a number and a fact, which
//! is what lets the projection apply it while folding the log. Whether a score
//! counts as mastered is not here: that is a threshold the doctrine pack states
//! (doc 17 section 8), and a pack is not something this layer can read.
//!
//! Doc 14's `score_mastery` counted correct checks in one session and is gone
//! from the write path. What the Tutor still reads under that name is the same
//! count, derived from the session's own checks rather than stored beside them,
//! so the eight `learn.*` shapes did not have to change for this.

/// Doc 17 section 2.4: "exposure adds 0.02 up to 0.2".
pub const EXPOSURE_STEP: f64 = 0.02;
pub const EXPOSURE_CAP: f64 = 0.20;

/// Doc 17 section 2.4's honesty rule: "a rating can never move mastery above
/// 0.5; only checks can". A claim about oneself is where learning starts and
/// not evidence that it happened.
pub const RATING_CAP: f64 = 0.5;

/// How fast a check at this level moves the score. Doc 17 section 2.4: 0.15 at
/// level 1, 0.35 at level 4, and the two between them are the line those two
/// points make rather than numbers picked to sit there.
pub fn k_for(level: u8) -> f64 {
    let level = level.clamp(1, 4) as f64;
    0.15 + (level - 1.0) * (0.35 - 0.15) / 3.0
}

/// Doc 17 section 2.4's prior: 0, 0.15, 0.35, 0.5 for ratings 0 to 3.
pub fn prior_for(rating: i64) -> f64 {
    let prior: f64 = match rating {
        0 => 0.0,
        1 => 0.15,
        2 => 0.35,
        _ => 0.5,
    };
    prior.min(RATING_CAP)
}

/// The score after a check. `repeated` halves the gain on a pass, because
/// answering the same item again is weaker evidence than answering a new one
/// and doc 17 section 2.4 asks for the reduction without naming a size.
///
/// A failure is not halved. The second time someone gets the same item wrong
/// says more than the first, not less.
pub fn after_check(mastery: Option<f64>, level: u8, correct: bool, repeated: bool) -> f64 {
    let current = mastery.unwrap_or(0.0);
    let k = if correct && repeated {
        k_for(level) / 2.0
    } else {
        k_for(level)
    };
    let outcome = if correct { 1.0 } else { 0.0 };
    (current + k * (outcome - current)).clamp(0.0, 1.0)
}

/// The score after reading a card that names the concept.
///
/// Capped, so exposure alone can never carry a concept past 0.2: reading about
/// something is not knowing it, and a map where browsing looked like learning
/// would be worth nothing to the person reading it.
pub fn after_exposure(mastery: Option<f64>) -> f64 {
    let current = mastery.unwrap_or(0.0);
    if current >= EXPOSURE_CAP {
        current
    } else {
        (current + EXPOSURE_STEP).min(EXPOSURE_CAP)
    }
}

/// The score after a self rating, or `None` when the rating changes nothing.
///
/// Doc 17 section 2.4: the prior is "applied only when mastery is null". A
/// rating after evidence exists says what the learner believes about themselves
/// and the evidence already says what happened.
pub fn after_rating(mastery: Option<f64>, rating: i64) -> Option<f64> {
    match mastery {
        Some(_) => None,
        None => Some(prior_for(rating)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_moves_the_score_towards_its_outcome() {
        // Doc 17 section 2.4's formula, at the two levels it names.
        assert!((k_for(1) - 0.15).abs() < 1e-9);
        assert!((k_for(4) - 0.35).abs() < 1e-9);
        assert!(k_for(2) > k_for(1) && k_for(3) < k_for(4));

        let first = after_check(None, 1, true, false);
        assert!((first - 0.15).abs() < 1e-9, "a first pass at level 1 is 0.15");

        // Towards the outcome, never past it, however many times it is asked.
        let mut score = None;
        for _ in 0..100 {
            score = Some(after_check(score, 4, true, false));
        }
        assert!(score.expect("score") < 1.0);
        assert!(score.expect("score") > 0.99);
    }

    #[test]
    fn a_repeated_pass_counts_for_less_and_a_repeated_failure_does_not() {
        let fresh = after_check(Some(0.4), 3, true, false);
        let again = after_check(Some(0.4), 3, true, true);
        assert!(again < fresh, "answering the same item again moved it as far");

        assert_eq!(
            after_check(Some(0.4), 3, false, true),
            after_check(Some(0.4), 3, false, false),
            "the second wrong answer to one item says less than the first"
        );
    }

    #[test]
    fn exposure_cannot_carry_a_concept_past_a_fifth() {
        let mut score = None;
        for _ in 0..50 {
            score = Some(after_exposure(score));
        }
        assert!((score.expect("score") - EXPOSURE_CAP).abs() < 1e-9);

        // And it never pulls a checked concept back down to the cap.
        assert!((after_exposure(Some(0.9)) - 0.9).abs() < 1e-9);
    }

    #[test]
    fn a_rating_starts_a_score_and_never_moves_one() {
        // Doc 17 section 2.4: the prior applies only when mastery is null.
        assert_eq!(after_rating(None, 3), Some(0.5));
        assert_eq!(after_rating(None, 0), Some(0.0));
        assert_eq!(after_rating(Some(0.1), 3), None, "a rating overwrote evidence");

        // The honesty rule: no rating reaches past a half.
        for rating in 0..=3 {
            assert!(prior_for(rating) <= RATING_CAP);
        }
    }
}

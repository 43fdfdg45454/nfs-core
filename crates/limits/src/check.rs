use std::fmt;
use std::time::Duration;

/// Mean and worst case of a set of timings, as reported in the CI annotations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub count: usize,
    pub mean: Duration,
    pub max: Duration,
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        write!(f, "{} × mean {:.0} ms max {:.0} ms", self.count, ms(self.mean), ms(self.max))
    }
}

/// Summarizes `samples` and fails, naming what was measured, when the mean exceeds `mean_limit`
/// (if any) or any sample exceeds `max_limit`.
pub fn check(
    what: &str,
    samples: &[Duration],
    mean_limit: Option<Duration>,
    max_limit: Duration,
) -> Result<Summary, String> {
    let Some(&max) = samples.iter().max() else {
        return Err(format!("{what}: nothing was measured"));
    };
    let mean = samples.iter().sum::<Duration>() / samples.len() as u32;
    let summary = Summary { count: samples.len(), mean, max };
    let slow_mean = mean_limit.is_some_and(|limit| mean > limit);
    if slow_mean || max > max_limit {
        return Err(format!("{what}: {summary}, limits mean {mean_limit:?} max {max_limit:?}"));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SEEK_MAX, SEEK_MEAN};

    const fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn within_limits() {
        let summary = check("seek", &[ms(200), ms(400)], Some(SEEK_MEAN), SEEK_MAX).unwrap();
        assert_eq!(summary, Summary { count: 2, mean: ms(300), max: ms(400) });
    }

    #[test]
    fn slow_mean_or_one_slow_sample_fails() {
        assert!(check("seek", &[ms(1100), ms(1200)], Some(SEEK_MEAN), SEEK_MAX).is_err());
        assert!(check("seek", &[ms(10), ms(10), ms(3100)], Some(SEEK_MEAN), SEEK_MAX).is_err());
    }

    #[test]
    fn nothing_measured_fails() {
        assert!(check("open", &[], None, crate::OPEN).is_err());
    }
}

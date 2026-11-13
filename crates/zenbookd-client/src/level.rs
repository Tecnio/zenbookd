#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Good,
    Low,
    Critical,
}

pub fn charge_level(percent: u32) -> Level {
    if percent <= 20 {
        Level::Critical
    } else if percent <= 50 {
        Level::Low
    } else {
        Level::Good
    }
}

pub fn health_level(percent: u32) -> Level {
    if percent < 80 {
        Level::Critical
    } else if percent < 90 {
        Level::Low
    } else {
        Level::Good
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_thresholds() {
        assert_eq!(charge_level(20), Level::Critical);
        assert_eq!(charge_level(21), Level::Low);
        assert_eq!(charge_level(50), Level::Low);
        assert_eq!(charge_level(51), Level::Good);
    }

    #[test]
    fn health_thresholds() {
        assert_eq!(health_level(79), Level::Critical);
        assert_eq!(health_level(80), Level::Low);
        assert_eq!(health_level(89), Level::Low);
        assert_eq!(health_level(90), Level::Good);
    }
}

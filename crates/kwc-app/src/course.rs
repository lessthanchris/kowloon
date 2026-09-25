//! Courses: a city seed and an era, written as a short code you can share
//! (`KWC-1965-7F3A`). The same code always gives the same city, the same
//! era and the same deliveries in the same order, so two people can race it.

use crate::game::ERAS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Course {
    pub seed: u16,
    pub era: u16,
}

impl Course {
    pub fn code(&self) -> String {
        format!("KWC-{}-{:04X}", self.era, self.seed)
    }

    /// Reads `KWC-1965-7F3A`, forgivingly: any case, spaces for dashes, the
    /// `KWC` optional. The era must be one of the game's eras.
    pub fn parse(s: &str) -> Option<Course> {
        let s = s.trim().to_uppercase().replace([' ', '_'], "-");
        let s = s.strip_prefix("KWC-").unwrap_or(&s);
        let (era, seed) = s.split_once('-')?;
        let era: u16 = era.parse().ok()?;
        let seed = u16::from_str_radix(seed, 16).ok()?;
        ERAS.contains(&era).then_some(Course { seed, era })
    }

    /// Today's course: the same for everyone on the same (UTC) day, cycling
    /// through the eras.
    pub fn daily() -> Course {
        let day = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400);
        Course::for_day(day)
    }

    fn for_day(day: u64) -> Course {
        let mut h = day.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        h ^= h >> 29;
        Course { seed: (h & 0xFFFF) as u16, era: ERAS[(day % ERAS.len() as u64) as usize] }
    }

    /// A fresh course, any era.
    pub fn random() -> Course {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        let c = Course::for_day(t);
        Course { seed: c.seed, era: ERAS[((t >> 20) % ERAS.len() as u64) as usize] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for c in [Course { seed: 0, era: 1950 }, Course { seed: 0x7F3A, era: 1965 }, Course { seed: 0xFFFF, era: 1987 }] {
            assert_eq!(Course::parse(&c.code()), Some(c));
        }
        assert_eq!(Course::parse(" kwc 1965 7f3a "), Some(Course { seed: 0x7F3A, era: 1965 }));
        assert_eq!(Course::parse("1965-7F3A"), Some(Course { seed: 0x7F3A, era: 1965 }));
        assert_eq!(Course::parse("KWC-1966-7F3A"), None, "not an era");
        assert_eq!(Course::parse("KWC-1965-XYZ"), None);
    }

    #[test]
    fn daily_is_stable_within_a_day() {
        assert_eq!(Course::for_day(20_000), Course::for_day(20_000));
        assert_ne!(Course::for_day(20_000), Course::for_day(20_001));
    }
}

//! Courses: an era and a delivery list, written as a short code you can
//! share. `KWC-1965-7F3A` is in **the** Walled City, the one canonical city
//! everyone learns (the campaign's). `KWX-1965-7F3A` is a *wild* city, grown
//! fresh from the code. The same code always gives the same city, era and
//! deliveries in the same order, so two people can race it.

use crate::game::ERAS;

/// The canonical city's seed: the campaign city, the one to learn.
pub const CITY: u64 = 1987;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Course {
    /// The delivery list (and, for a wild city, the city too).
    pub seed: u16,
    pub era: u16,
    /// A fresh city grown from the seed, rather than the canonical one.
    pub wild: bool,
}

impl Course {
    pub fn code(&self) -> String {
        format!("{}-{}-{:04X}", if self.wild { "KWX" } else { "KWC" }, self.era, self.seed)
    }

    /// The city this course runs in.
    pub fn city_seed(&self) -> u64 {
        if self.wild {
            self.seed as u64
        } else {
            CITY
        }
    }

    /// Reads `KWC-1965-7F3A` / `KWX-...`, forgivingly: any case, spaces for
    /// dashes, the prefix optional (no prefix means the canonical city). The
    /// era must be one of the game's eras.
    pub fn parse(s: &str) -> Option<Course> {
        let s = s.trim().to_uppercase().replace([' ', '_'], "-");
        let (wild, rest) = if let Some(r) = s.strip_prefix("KWX-") {
            (true, r)
        } else {
            (false, s.strip_prefix("KWC-").unwrap_or(&s))
        };
        let (era, seed) = rest.split_once('-')?;
        let era: u16 = era.parse().ok()?;
        let seed = u16::from_str_radix(seed, 16).ok()?;
        ERAS.contains(&era).then_some(Course { seed, era, wild })
    }

    /// Today's course, in the canonical city: the same for everyone on the
    /// same (UTC) day, cycling through the eras.
    pub fn daily() -> Course {
        let day = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400);
        Course::for_day(day)
    }

    fn for_day(day: u64) -> Course {
        Course { seed: hash16(day), era: ERAS[(day % ERAS.len() as u64) as usize], wild: false }
    }

    /// A fresh delivery list in `era`: in the canonical city, or a wild one.
    pub fn random(era: u16, wild: bool) -> Course {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        Course { seed: hash16(t), era, wild }
    }
}

fn hash16(x: u64) -> u16 {
    let mut h = x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 29;
    (h & 0xFFFF) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for c in [
            Course { seed: 0, era: 1950, wild: false },
            Course { seed: 0x7F3A, era: 1965, wild: false },
            Course { seed: 0xFFFF, era: 1987, wild: true },
        ] {
            assert_eq!(Course::parse(&c.code()), Some(c));
        }
        assert_eq!(Course::parse(" kwc 1965 7f3a "), Some(Course { seed: 0x7F3A, era: 1965, wild: false }));
        assert_eq!(Course::parse("1965-7F3A"), Some(Course { seed: 0x7F3A, era: 1965, wild: false }));
        assert_eq!(Course::parse("kwx-1965-7f3a"), Some(Course { seed: 0x7F3A, era: 1965, wild: true }));
        assert_eq!(Course::parse("KWC-1966-7F3A"), None, "not an era");
        assert_eq!(Course::parse("KWC-1965-XYZ"), None);
    }

    #[test]
    fn canonical_courses_share_one_city() {
        let a = Course::parse("KWC-1950-0001").unwrap();
        let b = Course::parse("KWC-1987-BEEF").unwrap();
        assert_eq!(a.city_seed(), b.city_seed());
        assert_eq!(a.city_seed(), CITY);
        assert_ne!(Course::parse("KWX-1987-BEEF").unwrap().city_seed(), CITY);
    }

    #[test]
    fn daily_is_stable_within_a_day() {
        assert_eq!(Course::for_day(20_000), Course::for_day(20_000));
        assert_ne!(Course::for_day(20_000), Course::for_day(20_001));
        assert!(!Course::for_day(20_000).wild, "the daily is in the canonical city");
    }
}

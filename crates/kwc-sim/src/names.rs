//! Names: Cantonese surnames and given names (Hong Kong romanisation), and shop
//! names by trade in the style of the period ("Wing Kee", "Tai Hing").

use crate::city::UnitUse;
use rand::seq::SliceRandom;
use rand::Rng;

/// Surname and rough frequency weight.
pub const SURNAMES: &[(&str, u32)] = &[
    ("Chan", 10), ("Wong", 9), ("Lee", 7), ("Cheung", 5), ("Lau", 4), ("Leung", 4), ("Lam", 4),
    ("Ho", 3), ("Ng", 3), ("Cheng", 2), ("Tang", 2), ("Yip", 2), ("Tsang", 2), ("Fung", 2),
    ("Kwok", 2), ("Law", 2), ("Chow", 2), ("Yeung", 2), ("Mak", 1), ("Tam", 1), ("Choi", 1),
    ("Lai", 1), ("So", 1), ("Siu", 1), ("Kwan", 1), ("Poon", 1), ("Yuen", 1), ("Ma", 1),
    ("Tse", 1), ("Lo", 1), ("Au", 1), ("Hui", 1),
];

const MALE: &[&str] = &[
    "Ka", "Wai", "Kwok", "Chi", "Man", "Kin", "Ming", "Tak", "Wing", "Hon", "Yiu", "Kam", "Chun",
    "Hung", "Keung", "Fai", "Ho", "Lok", "Shing", "Kwong", "Sun", "Pak", "Yat", "Hei", "Tsz",
];
const FEMALE: &[&str] = &[
    "Mei", "Yuk", "Siu", "Lai", "Wai", "Ka", "Suk", "Fung", "Yee", "Ling", "Man", "Ying", "Wah",
    "Kit", "Sau", "Po", "Mui", "Chu", "Lan", "Oi", "Hing", "Kuen", "Yin", "Sze", "Wan",
];

const LUCKY: &[&str] = &["Wing", "Hing", "Tai", "Kam", "Fook", "Yan", "Wah", "Shing", "On", "Lung", "Cheung", "Fat", "Kwong", "Yuen", "Lee"];
const SUFFIX: &[&str] = &["Kee", "Hing", "Lung", "On", "Tai", "Wo", "Cheong", "Fat"];

pub fn surname(rng: &mut impl Rng) -> &'static str {
    let total: u32 = SURNAMES.iter().map(|s| s.1).sum();
    let mut r = rng.gen_range(0..total);
    for &(s, w) in SURNAMES {
        if r < w {
            return s;
        }
        r -= w;
    }
    "Chan"
}

/// Two syllables, hyphenated, as on a Hong Kong ID card: "Ka-ming".
pub fn given(rng: &mut impl Rng, female: bool) -> String {
    let pool = if female { FEMALE } else { MALE };
    let a = pool.choose(rng).unwrap();
    let mut b = pool.choose(rng).unwrap();
    while b == a {
        b = pool.choose(rng).unwrap();
    }
    format!("{a}-{}", b.to_lowercase())
}

/// A trading name for a business of `trade` run by the `owner` family.
pub fn business(rng: &mut impl Rng, trade: UnitUse, owner: &str, owner_given: &str) -> String {
    let kee = format!("{} {}", LUCKY.choose(rng).unwrap(), SUFFIX.choose(rng).unwrap());
    let family = format!("{owner} Kee");
    let brand = if rng.gen_bool(0.5) { kee } else { family };
    use UnitUse::*;
    match trade {
        Shop => format!("{brand} {}", ["Store", "Groceries", "Rice & Oil", "Provisions", "Sundries", "Tobacco & Sweets"].choose(rng).unwrap()),
        Workshop => format!("{brand} {}", ["Metalworks", "Plastics", "Garments", "Watchstraps", "Printing", "Toys", "Sheet Metal", "Textiles"].choose(rng).unwrap()),
        FishballFactory => format!("{brand} {}", ["Fishballs", "Fish Products", "Noodles & Fishballs"].choose(rng).unwrap()),
        Dentist => {
            if rng.gen_bool(0.5) {
                format!("{owner} Dental Clinic")
            } else {
                format!("Dr {owner} {owner_given}, Dentist")
            }
        }
        Clinic => format!("{owner} {}", ["Medical Clinic", "Herbal Medicine", "Bone-setter"].choose(rng).unwrap()),
        Restaurant => format!("{brand} {}", ["Cafe", "Noodles", "Roast Meats", "Tea House", "Congee"].choose(rng).unwrap()),
        Temple | Flat => format!("{brand}"),
    }
}

//! Player preferences, kept in `settings.json` next to the game: mouse,
//! view, display, and which key does what.

use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

const FILE: &str = "settings.json";

/// Everything a key can be bound to. Esc (menu) and Tab (overview) are fixed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Jog,
    Jump,
    Knock,
    Notebook,
    Ledger,
    Memory,
    Torch,
    Night,
    Autopilot,
    MoveOn,
    Restart,
    Help,
    Vsync,
}

impl Action {
    pub const ALL: [Action; 17] = [
        Action::Forward,
        Action::Back,
        Action::Left,
        Action::Right,
        Action::Jog,
        Action::Jump,
        Action::Knock,
        Action::Notebook,
        Action::Ledger,
        Action::Memory,
        Action::Torch,
        Action::Night,
        Action::Autopilot,
        Action::MoveOn,
        Action::Restart,
        Action::Help,
        Action::Vsync,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::Forward => "Walk forward",
            Action::Back => "Walk back",
            Action::Left => "Step left",
            Action::Right => "Step right",
            Action::Jog => "Jog (hold)",
            Action::Jump => "Hop / vault",
            Action::Knock => "Knock",
            Action::Notebook => "Notebook map",
            Action::Ledger => "Ledger",
            Action::Memory => "Memory mode",
            Action::Torch => "Torch",
            Action::Night => "Night",
            Action::Autopilot => "Autopilot",
            Action::MoveOn => "Move on an era",
            Action::Restart => "Restart race",
            Action::Help => "Help card",
            Action::Vsync => "Vsync",
        }
    }

    fn default_key(self) -> KeyCode {
        match self {
            Action::Forward => KeyCode::KeyW,
            Action::Back => KeyCode::KeyS,
            Action::Left => KeyCode::KeyA,
            Action::Right => KeyCode::KeyD,
            Action::Jog => KeyCode::ShiftLeft,
            Action::Jump => KeyCode::Space,
            Action::Knock => KeyCode::KeyE,
            Action::Notebook => KeyCode::KeyM,
            Action::Ledger => KeyCode::KeyL,
            Action::Memory => KeyCode::KeyG,
            Action::Torch => KeyCode::KeyT,
            Action::Night => KeyCode::KeyN,
            Action::Autopilot => KeyCode::KeyP,
            Action::MoveOn => KeyCode::KeyY,
            Action::Restart => KeyCode::KeyR,
            Action::Help => KeyCode::KeyH,
            Action::Vsync => KeyCode::KeyV,
        }
    }
}

/// Keys that can be bound, by name (so settings.json stays readable).
const BINDABLE: &[KeyCode] = &[
    KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF, KeyCode::KeyG,
    KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM, KeyCode::KeyN,
    KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR, KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU,
    KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY, KeyCode::KeyZ,
    KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4,
    KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
    KeyCode::Space, KeyCode::ShiftLeft, KeyCode::ShiftRight, KeyCode::ControlLeft, KeyCode::ControlRight,
    KeyCode::AltLeft, KeyCode::AltRight, KeyCode::Enter, KeyCode::Backspace, KeyCode::CapsLock,
    KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::ArrowLeft, KeyCode::ArrowRight,
    KeyCode::Comma, KeyCode::Period, KeyCode::Slash, KeyCode::Semicolon, KeyCode::Quote,
    KeyCode::BracketLeft, KeyCode::BracketRight, KeyCode::Minus, KeyCode::Equal, KeyCode::Backquote, KeyCode::Backslash,
    KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5, KeyCode::F6,
    KeyCode::F7, KeyCode::F8, KeyCode::F9, KeyCode::F10, KeyCode::F12,
    KeyCode::Numpad0, KeyCode::Numpad1, KeyCode::Numpad2, KeyCode::Numpad3, KeyCode::Numpad4,
    KeyCode::Numpad5, KeyCode::Numpad6, KeyCode::Numpad7, KeyCode::Numpad8, KeyCode::Numpad9,
];

pub fn key_name(k: KeyCode) -> String {
    let s = format!("{k:?}");
    s.strip_prefix("Key").or_else(|| s.strip_prefix("Digit")).unwrap_or(&s).to_string()
}

fn key_from_name(name: &str) -> Option<KeyCode> {
    BINDABLE.iter().copied().find(|&k| key_name(k) == name)
}

pub fn bindable(k: KeyCode) -> bool {
    BINDABLE.contains(&k)
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Prefs {
    /// Mouse look speed (1 = the original).
    pub sensitivity: f32,
    pub invert_y: bool,
    /// Vertical field of view, degrees.
    pub fov: f32,
    /// Head bob strength (0 = off, 1 = normal).
    pub head_bob: f32,
    pub vsync: bool,
    pub fullscreen: bool,
    /// Key for each action, by key name.
    keys: Vec<(Action, String)>,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            sensitivity: 1.0,
            invert_y: false,
            fov: 72.0,
            head_bob: 1.0,
            vsync: true,
            fullscreen: false,
            keys: Action::ALL.iter().map(|&a| (a, key_name(a.default_key()))).collect(),
        }
    }
}

impl Prefs {
    /// Loaded from settings.json; anything missing or unreadable falls back
    /// to the defaults. The flag says whether this is a first run.
    pub fn load() -> (Prefs, bool) {
        match std::fs::read_to_string(FILE).ok().and_then(|s| serde_json::from_str::<Prefs>(&s).ok()) {
            Some(mut p) => {
                // Actions added since the file was written get their defaults.
                for a in Action::ALL {
                    if !p.keys.iter().any(|(b, _)| *b == a) {
                        p.keys.push((a, key_name(a.default_key())));
                    }
                }
                (p, false)
            }
            None => (Prefs::default(), true),
        }
    }

    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(FILE, json);
        }
    }

    pub fn key(&self, a: Action) -> KeyCode {
        self.keys.iter().find(|(b, _)| *b == a).and_then(|(_, n)| key_from_name(n)).unwrap_or(a.default_key())
    }

    /// The action on a key, if any. The Jog action also answers to the other Shift.
    pub fn action(&self, k: KeyCode) -> Option<Action> {
        Action::ALL.into_iter().find(|&a| self.key(a) == k)
    }

    /// Put `a` on key `k`; whatever had `k` takes `a`'s old key (a swap), so
    /// nothing is ever left unbound or doubled up.
    pub fn bind(&mut self, a: Action, k: KeyCode) {
        let old = self.key(a);
        let set = |keys: &mut Vec<(Action, String)>, a: Action, k: KeyCode| {
            keys.retain(|(b, _)| *b != a);
            keys.push((a, key_name(k)));
        };
        if let Some(other) = self.action(k).filter(|&o| o != a) {
            set(&mut self.keys, other, old);
        }
        set(&mut self.keys, a, k);
    }

    pub fn reset_keys(&mut self) {
        self.keys = Prefs::default().keys;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebinding_swaps_and_round_trips() {
        let mut p = Prefs::default();
        assert_eq!(p.action(KeyCode::KeyW), Some(Action::Forward));
        // Put Knock on F (free): E is now free.
        p.bind(Action::Knock, KeyCode::KeyF);
        assert_eq!(p.key(Action::Knock), KeyCode::KeyF);
        assert_eq!(p.action(KeyCode::KeyE), None);
        // Put Forward on T (Torch's): Torch takes W.
        p.bind(Action::Forward, KeyCode::KeyT);
        assert_eq!(p.key(Action::Forward), KeyCode::KeyT);
        assert_eq!(p.key(Action::Torch), KeyCode::KeyW);
        let back: Prefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.key(Action::Forward), KeyCode::KeyT);
        assert_eq!(back.key(Action::Knock), KeyCode::KeyF);
        for a in Action::ALL {
            assert_eq!(Action::ALL.iter().filter(|&&b| back.key(b) == back.key(a)).count(), 1, "{a:?} shares a key");
        }
    }
}

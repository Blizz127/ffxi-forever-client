//! Ours (owner: "remove all PlayOnline references in the Forever client"): FFXI Forever shows the game's own text with
//! its PlayOnline wording put another way. Display only: the DATs are read as they are, the words change as a message
//! table hands an entry out. Regular FFXI keeps retail's words.

use std::sync::atomic::{AtomicBool, Ordering};

static FOREVER: AtomicBool = AtomicBool::new(false);

/// The server is FFXI Forever (set from the config whenever its server changes).
pub fn set_forever(on: bool) {
    FOREVER.store(on, Ordering::Relaxed);
}

pub fn forever() -> bool {
    FOREVER.load(Ordering::Relaxed)
}

/// Generic rules, not retail's lines: the word "PlayOnline" with the plain English around it (prepositions, a
/// possessive, the game's name) put another way, most specific first; then the word alone in any language. Our own
/// updater line comes first.
const PHRASES: &[(&str, &str)] = &[
    ("the PlayOnline patch server", "Square Enix's patch server"),
    ("PlayOnline FINAL FANTASY XI", "FINAL FANTASY XI"),
    (" (PlayOnline)", ""),
    ("PlayOnline viewer's ", ""),
    ("PlayOnline Viewer", "game"),
    ("PlayOnline's ", ""),
    (" and PlayOnline", ""),
    ("through PlayOnline", "online"),
    ("to\nPlayOnline", "to\nthe title screen"),
    ("PlayOnline and ", "the game and "),
    ("from PlayOnline", "from the game"),
    // Before a noun the word just goes ("the PlayOnline list" -> "the list"); what is left ends a phrase.
    ("PlayOnline ", ""),
    ("to PlayOnline", "to the title screen"),
    ("of PlayOnline", "of the game"),
    ("PlayOnline-", ""),
    (" PlayOnline", " the game"),
    ("PlayOnline", "FFXI"),
];

/// The text as FFXI Forever shows it (bytes: Shift-JIS keeps ASCII as it is).
pub fn bytes(text: Vec<u8>) -> Vec<u8> {
    if !forever() || !contains(&text, b"PlayOnline") {
        return text;
    }
    let mut t = text;
    for (from, to) in PHRASES {
        if contains(&t, from.as_bytes()) {
            t = replace(&t, from.as_bytes(), to.as_bytes());
        }
    }
    t
}

/// The same for our own text.
pub fn text(text: String) -> String {
    if !forever() || !text.contains("PlayOnline") {
        return text;
    }
    String::from_utf8(bytes(text.into_bytes())).unwrap_or_default()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn replace(hay: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> String {
        String::from_utf8(
            PHRASES.iter().fold(s.as_bytes().to_vec(), |t, (a, b)| replace(&t, a.as_bytes(), b.as_bytes())),
        )
        .unwrap()
    }

    #[test]
    fn lines_read_without_playonline() {
        // Made-up sentences (not the game's): each rule and the order between them.
        let lines = [
            ("Go back to PlayOnline?", "Go back to the title screen?"),
            ("Open the PlayOnline list.", "Open the list."),
            ("Leave FINAL FANTASY XI and PlayOnline.", "Leave FINAL FANTASY XI."),
            ("A note (PlayOnline) here.", "A note here."),
            ("Settings for all of PlayOnline.", "Settings for all of the game."),
            ("Sign out from PlayOnline now.", "Sign out from the game now."),
            ("Reached through PlayOnline.", "Reached online."),
            ("The PlayOnline FINAL FANTASY XI page.", "The FINAL FANTASY XI page."),
            ("Use PlayOnline's slot.", "Use slot."),
            ("Please restart PlayOnline", "Please restart the game"),
            ("zum PlayOnline-Server", "zum Server"),
            ("Sign out from PlayOnline and rest.", "Sign out from the game and rest."),
            ("Copy it to PlayOnline storage.", "Copy it to storage."),
        ];
        for (from, to) in lines {
            assert_eq!(fx(from), to, "{from:?}");
            assert!(!fx(from).contains("PlayOnline"));
        }
    }

    #[test]
    fn off_or_unrelated_text_is_untouched() {
        set_forever(false);
        assert_eq!(bytes(b"Go back to PlayOnline?".to_vec()), b"Go back to PlayOnline?");
        assert_eq!(text("Hello".into()), "Hello");
    }
}

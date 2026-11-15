//! The classic flavor keeps the game's original wording. No Forever replacements are compiled.

pub fn set_forever(_on: bool) {}

pub fn bytes(text: Vec<u8>) -> Vec<u8> {
    text
}

pub fn text(text: String) -> String {
    text
}

#[cfg(test)]
mod tests {
    #[test]
    fn classic_keeps_original_wording() {
        super::set_forever(true);
        let text = "Return to PlayOnline";
        assert_eq!(super::text(text.into()), text);
        assert_eq!(super::bytes(text.as_bytes().to_vec()), text.as_bytes());
    }
}

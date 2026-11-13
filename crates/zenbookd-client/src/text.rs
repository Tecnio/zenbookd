pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_control_characters_and_whitespace() {
        assert_eq!(sanitize("bad\x1b[31m\n  value\t"), "bad [31m value");
    }
}

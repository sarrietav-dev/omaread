//! A voice a TTS engine can use, and how its name reads on screen.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    /// What the engine takes as a voice: a speech-dispatcher name or model
    /// path, or a Kokoro voice id such as `af_heart`.
    pub name: String,
    pub language: String,
    pub variant: String,
}

impl Voice {
    /// A Kokoro id as a name (`af_bella` → "Bella"); a Piper model path as
    /// "lessac (medium)"; anything else as it is.
    pub fn label(&self) -> String {
        if let Some(label) = kokoro_label(&self.name) {
            return label;
        }
        let file = self.name.rsplit('/').next().unwrap_or(&self.name);
        let stem = file.strip_suffix(".onnx").unwrap_or(file);
        let mut parts = stem.split('-');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(_lang), Some(speaker), Some(quality)) => format!("{speaker} ({quality})"),
            _ => stem.to_string(),
        }
    }
}

/// `af_bella` → "Bella". Only Kokoro's two-letter language/gender prefixes
/// qualify, so a Piper model path is left alone.
fn kokoro_label(name: &str) -> Option<String> {
    if name.contains(['/', '.']) {
        return None;
    }
    let (prefix, rest) = name.split_once('_')?;
    if !matches!(
        prefix,
        "af" | "am" | "bf" | "bm" | "ef" | "em" | "ff" | "hf" | "hm" | "if" | "im" | "jf" | "jm" | "pf" | "pm" | "zf" | "zm"
    ) {
        return None;
    }
    let mut words = Vec::new();
    for word in rest.split('_').filter(|w| !w.is_empty()) {
        let mut chars = word.chars();
        let Some(first) = chars.next() else { continue };
        words.push(format!("{}{}", first.to_uppercase(), chars.as_str()));
    }
    Some(words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piper_voices_get_readable_names() {
        let v = Voice {
            name: "/usr/share/piper-voices/en/en_US/lessac/medium/en_US-lessac-medium.onnx".into(),
            language: "en".into(),
            variant: "MALE1".into(),
        };
        assert_eq!(v.label(), "lessac (medium)");
    }

    #[test]
    fn kokoro_voices_read_as_their_names() {
        let v = Voice { name: "af_bella".into(), language: "en-US".into(), variant: "female".into() };
        assert_eq!(v.label(), "Bella");
        let v = Voice { name: "zm_yunxi".into(), language: "zh".into(), variant: "male".into() };
        assert_eq!(v.label(), "Yunxi");
    }

    #[test]
    fn other_names_are_left_alone() {
        let v = Voice { name: "english".into(), language: String::new(), variant: String::new() };
        assert_eq!(v.label(), "english");
    }
}

/// The first retention policy is intentionally conservative. An empty ASR
/// result or a negative VAD result is not evidence that the original sound is
/// meaningless. Keep any non-zero PCM until a separately validated policy can
/// prove it safe to discard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    DiscardDigitalSilence,
    Preserve,
}

impl Retention {
    pub fn code(self) -> &'static str {
        match self {
            Self::DiscardDigitalSilence => "discard_digital_silence_v1",
            Self::Preserve => "pending_conservative_v1",
        }
    }
}

pub fn classify(mono: &[f32]) -> Retention {
    if mono.iter().all(|sample| *sample == 0.0) {
        Retention::DiscardDigitalSilence
    } else {
        Retention::Preserve
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weak_and_ambiguous_audio_survives_even_without_text() {
        assert_eq!(classify(&[0.0; 48_000]), Retention::DiscardDigitalSilence);
        assert_eq!(classify(&[0.0, 0.000_001, 0.0]), Retention::Preserve);
        assert_eq!(classify(&[0.0, f32::NAN]), Retention::Preserve);
    }
}

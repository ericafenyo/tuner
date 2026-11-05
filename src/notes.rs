//! Frequency → note name / cents, and bass tunings.

const NAMES: [&str; 12] = [
    "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
];

/// A note as a MIDI number (A4 = 69).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note(pub i32);

impl Note {
    pub fn name(self) -> &'static str {
        NAMES[self.0.rem_euclid(12) as usize]
    }

    pub fn octave(self) -> i32 {
        self.0.div_euclid(12) - 1
    }

    pub fn freq(self, a4: f32) -> f32 {
        a4 * 2f32.powf((self.0 - 69) as f32 / 12.0)
    }
}

/// Signed distance in cents from `target` to `freq`.
pub fn cents(freq: f32, target: f32) -> f32 {
    1200.0 * (freq / target).log2()
}

/// Nearest equal-tempered note and the offset from it in cents.
pub fn nearest_note(freq: f32, a4: f32) -> (Note, f32) {
    let midi = 69.0 + 12.0 * (freq / a4).log2();
    let note = Note(midi.round() as i32);
    (note, cents(freq, note.freq(a4)))
}

pub struct Tuning {
    pub name: &'static str,
    /// Strings from lowest to highest, as MIDI notes.
    pub strings: &'static [Note],
}

// MIDI: B0=23, C1=24, D1=26, E1=28, A1=33, D2=38, G2=43, C3=48
pub const TUNINGS: &[Tuning] = &[
    Tuning {
        name: "Standard (E A D G)",
        strings: &[Note(28), Note(33), Note(38), Note(43)],
    },
    Tuning {
        name: "Drop D (D A D G)",
        strings: &[Note(26), Note(33), Note(38), Note(43)],
    },
    Tuning {
        name: "D Standard (D G C F)",
        strings: &[Note(26), Note(31), Note(36), Note(41)],
    },
    Tuning {
        name: "Half step down (E♭ A♭ D♭ G♭)",
        strings: &[Note(27), Note(32), Note(37), Note(42)],
    },
    Tuning {
        name: "5-string (B E A D G)",
        strings: &[Note(23), Note(28), Note(33), Note(38), Note(43)],
    },
    Tuning {
        name: "5-string high C (E A D G C)",
        strings: &[Note(28), Note(33), Note(38), Note(43), Note(48)],
    },
    Tuning {
        name: "6-string (B E A D G C)",
        strings: &[Note(23), Note(28), Note(33), Note(38), Note(43), Note(48)],
    },
];

impl Tuning {
    /// Index of the string closest (in pitch) to `freq`.
    pub fn closest_string(&self, freq: f32, a4: f32) -> usize {
        self.strings
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = cents(freq, a.freq(a4)).abs();
                let db = cents(freq, b.freq(a4)).abs();
                da.total_cmp(&db)
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_string_frequencies() {
        let s = TUNINGS[0].strings;
        let f: Vec<f32> = s.iter().map(|n| n.freq(440.0)).collect();
        for (got, want) in f.iter().zip([41.203, 55.0, 73.416, 97.999]) {
            assert!((got - want).abs() < 0.01, "{got} vs {want}");
        }
        assert_eq!((s[0].name(), s[0].octave()), ("E", 1));
        assert_eq!(Note(23).name(), "B");
        assert_eq!(Note(23).octave(), 0);
    }

    #[test]
    fn nearest_note_and_cents() {
        let (n, c) = nearest_note(41.0, 440.0);
        assert_eq!(n, Note(28));
        assert!((c - -8.57).abs() < 0.1, "{c}");
        let (n, _) = nearest_note(432.0, 432.0);
        assert_eq!(n, Note(69));
    }

    #[test]
    fn closest_string_picks_right_one() {
        let t = &TUNINGS[0];
        assert_eq!(t.closest_string(40.0, 440.0), 0);
        assert_eq!(t.closest_string(60.0, 440.0), 1);
        assert_eq!(t.closest_string(100.0, 440.0), 3);
    }
}

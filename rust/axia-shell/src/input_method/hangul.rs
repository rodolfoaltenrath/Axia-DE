use super::tables::{CHO, FINAL_PAIRS, JONG, VOWEL_PAIRS};
use super::{append_cp, character, text, KeyInfo, Output, Ui};
#[derive(Default, Clone, Copy)]
struct Syllable {
    initial: u32,
    vowel: u32,
    final_: u32,
}
#[derive(Default)]
pub(super) struct Hangul {
    syllable: Syllable,
    history: Vec<Syllable>,
}
fn jamo(c: u8) -> u32 {
    match c {
        b'q' => 0x3142,
        b'Q' => 0x3143,
        b'w' => 0x3148,
        b'W' => 0x3149,
        b'e' => 0x3137,
        b'E' => 0x3138,
        b'r' => 0x3131,
        b'R' => 0x3132,
        b't' => 0x3145,
        b'T' => 0x3146,
        b'o' => 0x3150,
        b'O' => 0x3152,
        b'p' => 0x3154,
        b'P' => 0x3156,
        _ => b"yuiasdfghjklzxcvbnm"
            .iter()
            .position(|&key| key == c.to_ascii_lowercase())
            .map_or(0, |i| {
                [
                    0x315b, 0x3155, 0x3151, 0x3141, 0x3134, 0x3147, 0x3139, 0x314e, 0x3157, 0x3153,
                    0x314f, 0x3163, 0x314b, 0x314c, 0x314a, 0x314d, 0x3160, 0x315c, 0x3161,
                ][i]
            }),
    }
}
fn combine(table: &[[u32; 3]], a: u32, b: u32) -> u32 {
    table
        .iter()
        .find(|row| row[0] == a && row[1] == b)
        .map_or(0, |row| row[2])
}
impl Hangul {
    pub(super) fn empty(&self) -> bool {
        self.syllable.initial == 0 && self.syllable.vowel == 0 && self.syllable.final_ == 0
    }
    pub(super) fn reset(&mut self) {
        self.syllable = Syllable::default();
        self.history.clear();
    }
    fn value(&self) -> Vec<u8> {
        let s = self.syllable;
        let initial = CHO.iter().position(|&c| c == s.initial);
        let final_ = if s.final_ == 0 {
            Some(0)
        } else {
            JONG.iter().position(|&c| c == s.final_)
        };
        let mut out = Vec::new();
        if s.initial != 0 && s.vowel != 0 && initial.is_some() && final_.is_some() {
            append_cp(
                &mut out,
                16,
                0xac00
                    + ((initial.unwrap() as u32 * 21 + s.vowel - 0x314f) * 28)
                    + final_.unwrap() as u32,
            );
        } else {
            for cp in [s.initial, s.vowel, s.final_] {
                if cp != 0 {
                    append_cp(&mut out, 16, cp);
                }
            }
        }
        out
    }
    fn show(&self, ui: &mut Ui<'_>) {
        ui.preedit(&self.value());
    }
    pub(super) fn commit(&mut self, out: &mut Output, ui: &mut Ui<'_>) {
        out.append(&self.value());
        self.reset();
        ui.preedit(b"");
    }
    fn push(&mut self) {
        if self.history.len() == 12 {
            self.history.remove(0);
        }
        self.history.push(self.syllable);
    }
    pub(super) fn key(&mut self, key: &KeyInfo, out: &mut Output, ui: &mut Ui<'_>) -> bool {
        if text(&key.key) == b"Backspace" {
            if self.empty() {
                return false;
            }
            self.syllable = self.history.pop().unwrap_or_default();
            self.show(ui);
            return true;
        }
        let c = character(key);
        let j = if c.is_ascii_alphabetic() { jamo(c) } else { 0 };
        if j == 0 {
            if !self.empty() {
                self.commit(out, ui);
            }
            return false;
        }
        if (0x314f..=0x3163).contains(&j) {
            if self.syllable.final_ != 0 {
                let mut keep = 0;
                let mut moved = self.syllable.final_;
                for row in FINAL_PAIRS {
                    if row[2] == self.syllable.final_ {
                        keep = row[0];
                        moved = row[1];
                    }
                }
                if CHO.contains(&moved) {
                    self.syllable.final_ = keep;
                    self.commit(out, ui);
                    self.syllable.initial = moved;
                    self.push();
                    self.syllable.vowel = j;
                    self.show(ui);
                    return true;
                }
            }
            let joined = if self.syllable.vowel != 0 && self.syllable.final_ == 0 {
                combine(VOWEL_PAIRS, self.syllable.vowel, j)
            } else {
                0
            };
            if joined != 0 {
                self.push();
                self.syllable.vowel = joined;
            } else if self.syllable.vowel == 0 && self.syllable.final_ == 0 {
                self.push();
                self.syllable.vowel = j;
            } else {
                self.commit(out, ui);
                self.syllable.vowel = j;
            }
        } else if self.syllable.initial != 0
            && self.syllable.vowel != 0
            && self.syllable.final_ == 0
            && JONG.iter().position(|&c| c == j).is_some_and(|i| i > 0)
        {
            self.push();
            self.syllable.final_ = j;
        } else if self.syllable.final_ != 0 && combine(FINAL_PAIRS, self.syllable.final_, j) != 0 {
            self.push();
            self.syllable.final_ = combine(FINAL_PAIRS, self.syllable.final_, j);
        } else if self.syllable.initial == 0 && self.syllable.vowel == 0 {
            self.push();
            self.syllable.initial = j;
        } else {
            self.commit(out, ui);
            self.syllable.initial = j;
        }
        self.show(ui);
        true
    }
}

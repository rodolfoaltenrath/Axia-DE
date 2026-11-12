use super::{
    add_candidate, append, append_cp, character,
    dictionary::{self, Skk},
    drop_last,
    tables::{FUNCTION_WORDS, ROMAJI},
    text, KeyInfo, Output, Ui,
};
const MAX_SEGMENTS: usize = 24;
const SEG_CANDIDATES: usize = 24;
#[derive(Default)]
struct Segment {
    start: usize,
    len: usize,
    okuri: i32,
    selected: usize,
    candidates: Vec<Vec<u8>>,
}
#[derive(Default)]
pub(super) struct Kana {
    kana: Vec<u8>,
    pending: Vec<u8>,
    dict: Option<Skk>,
    tried: bool,
    segments: Vec<Segment>,
    focused: usize,
    codepoints: Vec<u32>,
}
fn katakana(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in String::from_utf8_lossy(input).chars() {
        let mut cp = ch as u32;
        if (0x3041..=0x3096).contains(&cp) {
            cp += 0x60;
        }
        append_cp(&mut out, 768, cp);
    }
    out
}
fn okuri_letter(cp: u32) -> u8 {
    if cp == 0x3063 {
        return b't';
    }
    if cp == 0x3093 {
        return 0;
    }
    let Some(ch) = char::from_u32(cp) else {
        return 0;
    };
    let mut bytes = [0u8; 4];
    let kana = ch.encode_utf8(&mut bytes);
    ROMAJI
        .iter()
        .find(|(roman, value)| *value == kana && roman.as_bytes()[0].is_ascii_alphabetic())
        .map_or(0, |(roman, _)| roman.as_bytes()[0])
}
fn particle(cp: u32) -> bool {
    [
        0x306f, 0x304c, 0x3092, 0x306b, 0x3067, 0x3068, 0x306e, 0x3078, 0x3082, 0x3084, 0x306d,
        0x3088,
    ]
    .contains(&cp)
}
fn function_word(reading: &[u8]) -> bool {
    FUNCTION_WORDS.iter().any(|word| word.as_bytes() == reading)
}
impl Kana {
    #[cfg(test)]
    pub(super) fn for_test(data: Option<Vec<u8>>) -> Self {
        Self {
            dict: data.map(Skk::parse),
            tried: true,
            ..Self::default()
        }
    }
    pub(super) fn load(&mut self) -> bool {
        if !self.tried {
            self.tried = true;
            self.dict = dictionary::load(dictionary::SKK_NAMES).map(Skk::parse);
        }
        self.dict.is_some()
    }
    pub(super) fn empty(&self) -> bool {
        self.kana.is_empty() && self.pending.is_empty()
    }
    pub(super) fn reset_text(&mut self) {
        self.kana.clear();
        self.pending.clear();
    }
    fn flush(&mut self, final_: bool) {
        while !self.pending.is_empty() {
            let first = self.pending[0];
            if self.pending.len() >= 2
                && first == self.pending[1]
                && first != b'n'
                && first.is_ascii_alphabetic()
                && !b"aeiou".contains(&first)
            {
                append(&mut self.kana, 384, "っ".as_bytes());
                self.pending.remove(0);
                continue;
            }
            if first == b'n' && self.pending.len() >= 2 && !b"aeiouyn'".contains(&self.pending[1]) {
                append(&mut self.kana, 384, "ん".as_bytes());
                self.pending.remove(0);
                continue;
            }
            let exact = ROMAJI
                .iter()
                .find(|(roman, _)| roman.as_bytes() == self.pending);
            let more = ROMAJI.iter().any(|(roman, _)| {
                roman.as_bytes().starts_with(&self.pending) && roman.len() > self.pending.len()
            });
            if more && !final_ {
                return;
            }
            if let Some((_, value)) = exact {
                append(&mut self.kana, 384, value.as_bytes());
                self.pending.clear();
                return;
            }
            if first == b'n' {
                append(&mut self.kana, 384, "ん".as_bytes());
            } else {
                append(&mut self.kana, 384, &[first]);
            }
            self.pending.remove(0);
        }
    }
    fn show(&self, ui: &mut Ui<'_>) {
        let mut bytes = self.kana.clone();
        bytes.extend_from_slice(&self.pending);
        ui.preedit(&bytes);
    }
    fn clear(&mut self, ui: &mut Ui<'_>) {
        self.reset_text();
        ui.clear();
        ui.preedit(b"");
    }
    fn sub(&self, start: usize, len: usize, cap: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for &cp in self.codepoints.iter().skip(start).take(len) {
            append_cp(&mut out, cap, cp);
        }
        out
    }
    fn exists(&self, value: &[u8], okuri: bool) -> bool {
        self.dict
            .as_ref()
            .is_some_and(|dict| dict.contains(value, okuri))
    }
    fn word_cost(&self, start: usize, len: usize) -> (i32, i32) {
        let (mut cost, kind) = self.word_cost_inner(start, len);
        if cost >= 0 && len >= 2 && self.codepoints[start + len - 1] == 0x3092 {
            cost += 1200;
        }
        (cost, kind)
    }
    fn word_cost_inner(&self, start: usize, len: usize) -> (i32, i32) {
        let reading = self.sub(start, len, 256);
        let square = (len * len) as i32;
        if function_word(&reading) {
            return (1000 - square, 2);
        }
        if len == 1 && particle(self.codepoints[start]) {
            return (700 - square, 0);
        }
        let mut okuri = false;
        if len >= 2 {
            let mut stem = self.sub(start, len - 1, 256);
            let letter = okuri_letter(self.codepoints[start + len - 1]);
            if letter != 0 && stem.len() + 2 < 256 {
                stem.push(letter);
                okuri = self.exists(&stem, true);
            }
        }
        let n1 = self.codepoints.get(start + len).copied().unwrap_or(0);
        let n2 = self.codepoints.get(start + len + 1).copied().unwrap_or(0);
        let verb_follows = n1 == 0x307e && [0x3059, 0x3057, 0x305b].contains(&n2);
        let mut starts_particle = false;
        let mut tie = 0;
        if start > 0 && (2..=4).contains(&len) && particle(self.codepoints[start]) {
            starts_particle = function_word(&self.sub(start + 1, len - 1, 256));
            tie = 5;
            let verbish =
                [0x3092, 0x3078, 0x306b, 0x3067, 0x304c].contains(&self.codepoints[start]);
            if !starts_particle && verbish && len >= 3 {
                let mut stem = self.sub(start + 1, len - 2, 256);
                let letter = okuri_letter(self.codepoints[start + len - 1]);
                if letter != 0 && stem.len() + 2 < 256 {
                    stem.push(letter);
                    starts_particle = self.exists(&stem, true);
                }
            }
        }
        let penalty = if starts_particle { 1100 } else { tie };
        if okuri && verb_follows {
            return (1000 - square - 30 + penalty, 1);
        }
        if verb_follows && !okuri {
            let mut stem = self.sub(start, len, 256);
            if stem.len() + 2 < 256 {
                stem.push(b'r');
                if self.exists(&stem, true) {
                    return (1000 - square - 30 + penalty, 3);
                }
            }
        }
        if self.exists(&reading, false) {
            let mut cost = (if len == 1 { 1400 } else { 1000 }) - square + penalty;
            if len >= 3
                && self.codepoints[start + len - 1] == 0x306f
                && self.exists(&self.sub(start, len - 1, 256), false)
            {
                cost += 1200;
            }
            return (cost, 0);
        }
        if okuri {
            return (1000 - square + penalty, 1);
        }
        (if len == 1 { 1500 - square } else { -1 }, 0)
    }
    fn segment_from(&mut self, from: usize) {
        let end = self.codepoints.len();
        let mut best = [1 << 28; 161];
        let mut previous = [0; 161];
        let mut kinds = [0; 161];
        best[from] = 0;
        for i in from..end {
            if best[i] >= 1 << 28 {
                continue;
            }
            for len in 1..=10.min(end - i) {
                let (cost, kind) = self.word_cost(i, len);
                if cost < 0 {
                    continue;
                }
                if best[i] + cost < best[i + len]
                    || (best[i] + cost == best[i + len] && i > previous[i + len])
                {
                    best[i + len] = best[i] + cost;
                    previous[i + len] = i;
                    kinds[i + len] = kind;
                }
            }
        }
        let mut stack = Vec::new();
        let mut j = end;
        while j > from && stack.len() < 160 {
            stack.push(j);
            j = previous[j];
        }
        let mut start = from;
        while let Some(end) = stack.pop() {
            if self.segments.len() == MAX_SEGMENTS {
                break;
            }
            self.segments.push(Segment {
                start,
                len: end - start,
                okuri: kinds[end],
                ..Segment::default()
            });
            start = end;
        }
        if self.segments.len() == MAX_SEGMENTS {
            let last = self.segments.last_mut().unwrap();
            if last.start + last.len < self.codepoints.len() {
                last.len = self.codepoints.len() - last.start;
            }
        }
    }
    fn segment_candidates(&mut self, index: usize) {
        let segment = &self.segments[index];
        let start = segment.start;
        let len = segment.len;
        let kind = segment.okuri;
        let reading = self.sub(start, len, 384);
        let kata = katakana(&reading);
        let mut list = Vec::new();
        if kind == 2 || (len == 1 && particle(self.codepoints[start])) {
            add_candidate(&mut list, SEG_CANDIDATES, &reading);
        }
        if let Some(dict) = &self.dict {
            if kind != 1 && kind != 3 {
                dict.candidates(&reading, false, b"", &mut list, SEG_CANDIDATES - 2);
            }
            if kind == 3 {
                let mut key = self.sub(start, len, 384);
                key.push(b'r');
                dict.candidates(&key, true, b"", &mut list, SEG_CANDIDATES - 2);
            }
            if kind == 1 {
                let mut stem = self.sub(start, len - 1, 384);
                let suffix = self.sub(start + len - 1, 1, 8);
                stem.push(okuri_letter(self.codepoints[start + len - 1]));
                dict.candidates(&stem, true, &suffix, &mut list, SEG_CANDIDATES - 2);
            }
        }
        add_candidate(&mut list, SEG_CANDIDATES, &reading);
        add_candidate(&mut list, SEG_CANDIDATES, &kata);
        self.segments[index].candidates = list;
    }
    fn conversion_show(&self, ui: &mut Ui<'_>) {
        let mut bytes = Vec::new();
        ui.state.focus_begin = 0;
        ui.state.focus_end = 0;
        for (i, segment) in self.segments.iter().enumerate() {
            if i == self.focused {
                ui.state.focus_begin = bytes.len() as i32;
            }
            append(&mut bytes, 512, &segment.candidates[segment.selected]);
            if i == self.focused {
                ui.state.focus_end = bytes.len() as i32;
            }
        }
        ui.preedit(&bytes);
        let focused = &self.segments[self.focused];
        ui.state.ncand = focused.candidates.len() as i32;
        for (index, candidate) in focused.candidates.iter().enumerate() {
            super::copy_text(&mut ui.state.cand[index], candidate);
        }
        ui.state.sel = focused.selected as i32;
        ui.state.converting = 1;
        *ui.changed = true;
    }
    fn convert(&mut self, ui: &mut Ui<'_>) {
        self.flush(true);
        if self.kana.is_empty() {
            return;
        }
        ui.clear();
        self.codepoints = String::from_utf8_lossy(&self.kana)
            .chars()
            .take(160)
            .map(|ch| ch as u32)
            .collect();
        self.segments.clear();
        self.focused = 0;
        self.segment_from(0);
        for index in 0..self.segments.len() {
            self.segment_candidates(index);
        }
        self.conversion_show(ui);
    }
    fn resize(&mut self, delta: i32, ui: &mut Ui<'_>) {
        let focused = &self.segments[self.focused];
        let len = focused.len as i32 + delta;
        if len < 1 || focused.start + len as usize > self.codepoints.len() {
            return;
        }
        let start = focused.start;
        let len = len as usize;
        let kind = self.word_cost(start, len).1;
        let focused = &mut self.segments[self.focused];
        focused.len = len;
        focused.okuri = kind;
        focused.selected = 0;
        self.segments.truncate(self.focused + 1);
        if start + len < self.codepoints.len() {
            self.segment_from(start + len);
        }
        for index in self.focused..self.segments.len() {
            self.segment_candidates(index);
        }
        self.conversion_show(ui);
    }
    pub(super) fn commit(&mut self, out: &mut Output, ui: &mut Ui<'_>) {
        if ui.state.converting != 0 && !self.segments.is_empty() {
            for segment in &self.segments {
                out.append(&segment.candidates[segment.selected]);
            }
        } else {
            self.flush(true);
            out.append(&self.kana);
        }
        self.segments.clear();
        ui.state.focus_begin = 0;
        ui.state.focus_end = 0;
        self.clear(ui);
    }
    pub(super) fn key(&mut self, key: &KeyInfo, out: &mut Output, ui: &mut Ui<'_>) -> bool {
        let mut empty = self.empty();
        let name = text(&key.key);
        if ui.state.converting != 0 {
            if let Some(digit) = ui.digit(key) {
                self.segments[self.focused].selected = digit;
                if self.focused + 1 < self.segments.len() {
                    self.focused += 1;
                    self.conversion_show(ui);
                } else {
                    self.commit(out, ui);
                }
                return true;
            }
            match name {
                b" " | b"ArrowDown" => {
                    let focused = &mut self.segments[self.focused];
                    focused.selected = (focused.selected + 1) % focused.candidates.len();
                    self.conversion_show(ui);
                    return true;
                }
                b"ArrowUp" => {
                    let focused = &mut self.segments[self.focused];
                    focused.selected = (focused.selected + focused.candidates.len() - 1)
                        % focused.candidates.len();
                    self.conversion_show(ui);
                    return true;
                }
                b"ArrowRight" => {
                    if key.shift != 0 {
                        self.resize(1, ui);
                    } else if self.focused + 1 < self.segments.len() {
                        self.focused += 1;
                        self.conversion_show(ui);
                    }
                    return true;
                }
                b"ArrowLeft" => {
                    if key.shift != 0 {
                        self.resize(-1, ui);
                    } else if self.focused > 0 {
                        self.focused -= 1;
                        self.conversion_show(ui);
                    }
                    return true;
                }
                b"Enter" => {
                    self.commit(out, ui);
                    return true;
                }
                b"Escape" | b"Backspace" => {
                    ui.clear();
                    self.segments.clear();
                    ui.state.focus_begin = 0;
                    ui.state.focus_end = 0;
                    self.show(ui);
                    return true;
                }
                _ => {
                    self.commit(out, ui);
                    empty = true;
                }
            }
        }
        match name {
            b"Backspace" => {
                if empty {
                    return false;
                }
                if !self.pending.is_empty() {
                    self.pending.pop();
                } else {
                    drop_last(&mut self.kana);
                }
                self.show(ui);
                return true;
            }
            b"Enter" => {
                if empty {
                    return false;
                }
                self.commit(out, ui);
                return true;
            }
            b"Escape" => {
                if empty {
                    return false;
                }
                self.clear(ui);
                return true;
            }
            b" " => {
                if empty {
                    return false;
                }
                self.convert(ui);
                return true;
            }
            _ => {}
        }
        let c = character(key);
        let roman = c != 0 && (c.is_ascii_alphabetic() || c == b'\'' || b"-,.[]/?!".contains(&c));
        if !roman {
            if !empty {
                self.commit(out, ui);
            }
            return false;
        }
        append(&mut self.pending, 8, &[c.to_ascii_lowercase()]);
        self.flush(false);
        self.show(ui);
        true
    }
}

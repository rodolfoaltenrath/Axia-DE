use super::{
    append, character, dictionary, drop_last, text, KeyInfo, Output, Ui, MAX_CANDIDATES, PAGE,
};
use std::{cmp::Ordering, ops::Range};
struct Entry {
    text: Range<usize>,
    key: Range<usize>,
    pin: Range<usize>,
    weight: u32,
}
struct Abbreviation {
    initials: Vec<u8>,
    entry: usize,
}
pub(super) struct Dictionary {
    data: Vec<u8>,
    keys: Vec<u8>,
    entries: Vec<Entry>,
    abbreviations: Vec<Abbreviation>,
}
impl Dictionary {
    pub(super) fn parse(data: Vec<u8>) -> Self {
        let mut dict = Self {
            data,
            keys: Vec::new(),
            entries: Vec::new(),
            abbreviations: Vec::new(),
        };
        let start = (0..dict.data.len().saturating_sub(4))
            .find(|&i| &dict.data[i..i + 4] == b"\n...")
            .unwrap_or(dict.data.len().saturating_sub(4));
        let mut offset = start;
        for line in dict.data[start..].split(|&b| b == b'\n') {
            if let Some(tab) = line.iter().position(|&b| b == b'\t') {
                if line.first() != Some(&b'#') && dict.entries.len() < 1 << 17 {
                    let end = line[tab + 1..]
                        .iter()
                        .position(|&b| b == b'\t')
                        .map_or(line.len(), |i| tab + 1 + i);
                    let key_start = dict.keys.len();
                    for &byte in &line[tab + 1..end] {
                        if byte != b' ' && dict.keys.len() < (1 << 20) - 1 {
                            dict.keys.push(byte);
                        }
                    }
                    let key_len = (dict.keys.len() - key_start) as u16 as usize;
                    let text_len = tab as u16 as usize;
                    let pin_len = (end - tab - 1) as u16 as usize;
                    let weight = if end < line.len() {
                        let mut bytes = line[end + 1..].to_vec();
                        bytes.push(0);
                        unsafe {
                            libc::strtoul(bytes.as_ptr().cast(), std::ptr::null_mut(), 10) as u32
                        }
                    } else {
                        0
                    };
                    if key_len != 0 && text_len < 90 {
                        dict.entries.push(Entry {
                            text: offset..offset + text_len,
                            key: key_start..key_start + key_len,
                            pin: offset + tab + 1..offset + tab + 1 + pin_len,
                            weight,
                        });
                    }
                }
            }
            offset += line.len() + 1;
        }
        dict.entries.sort_by(|a, b| {
            dict.keys[a.key.clone()]
                .cmp(&dict.keys[b.key.clone()])
                .then_with(|| b.weight.cmp(&a.weight))
        });
        for (index, entry) in dict.entries.iter().enumerate() {
            let initials: Vec<u8> = dict.data[entry.pin.clone()]
                .split(|&b| b == b' ')
                .filter_map(|part| part.first().copied())
                .take(11)
                .collect();
            if (2..11).contains(&initials.len()) {
                dict.abbreviations.push(Abbreviation {
                    initials,
                    entry: index,
                });
            }
        }
        dict.abbreviations.sort_by(|a, b| {
            a.initials.cmp(&b.initials).then_with(|| {
                dict.entries[b.entry]
                    .weight
                    .cmp(&dict.entries[a.entry].weight)
            })
        });
        dict
    }
    fn lower(&self, key: &[u8]) -> usize {
        self.entries
            .partition_point(|entry| self.keys[entry.key.clone()].cmp(key) == Ordering::Less)
    }
}
pub(super) struct Pinyin {
    dict: Option<Dictionary>,
    tried: bool,
    raw: Vec<u8>,
    acc: Vec<u8>,
    consumed: [usize; MAX_CANDIDATES],
}
impl Default for Pinyin {
    fn default() -> Self {
        Self {
            dict: None,
            tried: false,
            raw: Vec::new(),
            acc: Vec::new(),
            consumed: [0; MAX_CANDIDATES],
        }
    }
}
impl Pinyin {
    #[cfg(test)]
    pub(super) fn for_test(data: Option<Vec<u8>>) -> Self {
        Self {
            dict: data.map(Dictionary::parse),
            tried: true,
            ..Self::default()
        }
    }
    pub(super) fn load(&mut self) -> (bool, bool) {
        if !self.tried {
            self.tried = true;
            self.dict = dictionary::load(dictionary::PINYIN_NAMES).map(Dictionary::parse);
        }
        (
            self.dict.is_some(),
            self.dict
                .as_ref()
                .is_some_and(|dict| !dict.entries.is_empty()),
        )
    }
    pub(super) fn reset_text(&mut self) {
        self.raw.clear();
        self.acc.clear();
    }
    pub(super) fn empty(&self) -> bool {
        self.raw.is_empty() && self.acc.is_empty()
    }
    fn clear(&mut self, ui: &mut Ui<'_>) {
        self.reset_text();
        ui.clear();
        ui.preedit(b"");
    }
    pub(super) fn commit_raw(&mut self, out: &mut Output, ui: &mut Ui<'_>) {
        out.append(&self.acc);
        out.append(&self.raw);
        self.clear(ui);
    }
    fn add(
        dict: &Dictionary,
        index: usize,
        consumed: usize,
        ui: &mut Ui<'_>,
        lengths: &mut [usize; MAX_CANDIDATES],
    ) {
        if ui.add(&dict.data[dict.entries[index].text.clone()]) {
            lengths[ui.state.ncand as usize - 1] = consumed;
        }
    }
    fn exact(&mut self, len: usize, ui: &mut Ui<'_>) {
        let Some(dict) = &self.dict else {
            return;
        };
        let key = &self.raw[..len];
        for index in dict.lower(key)..dict.entries.len() {
            if ui.state.ncand as usize >= MAX_CANDIDATES
                || dict.keys[dict.entries[index].key.clone()] != *key
            {
                break;
            }
            Self::add(dict, index, len, ui, &mut self.consumed);
        }
    }
    fn abbreviate(&mut self, ui: &mut Ui<'_>) {
        let Some(dict) = &self.dict else {
            return;
        };
        let mut tokens = Vec::new();
        let mut pos = 0;
        while pos < self.raw.len() && tokens.len() < 11 {
            let len = if self.raw.get(pos + 1) == Some(&b'h') && b"zcs".contains(&self.raw[pos]) {
                2
            } else {
                1
            };
            tokens.push(&self.raw[pos..pos + len]);
            pos += len;
        }
        if !(2..=10).contains(&tokens.len())
            || tokens.iter().any(|token| b"aeiouv".contains(&token[0]))
        {
            return;
        }
        let initials: Vec<u8> = tokens.iter().map(|token| token[0]).collect();
        let first = dict
            .abbreviations
            .partition_point(|entry| entry.initials < initials);
        let mut added = 0;
        for abbr in &dict.abbreviations[first..] {
            if abbr.initials != initials || added >= 24 || ui.state.ncand as usize >= MAX_CANDIDATES
            {
                break;
            }
            let entry = &dict.entries[abbr.entry];
            let syllables: Vec<&[u8]> = dict.data[entry.pin.clone()]
                .split(|&b| b == b' ')
                .filter(|part| !part.is_empty())
                .collect();
            if syllables.len() >= tokens.len()
                && syllables
                    .iter()
                    .zip(&tokens)
                    .all(|(syllable, token)| syllable.starts_with(token))
            {
                Self::add(dict, abbr.entry, self.raw.len(), ui, &mut self.consumed);
                added += 1;
            }
        }
    }
    fn complete(&mut self, ui: &mut Ui<'_>) {
        let Some(dict) = &self.dict else {
            return;
        };
        let mut best: Vec<usize> = Vec::new();
        for index in (dict.lower(&self.raw)..dict.entries.len()).take(20000) {
            let entry = &dict.entries[index];
            let key = &dict.keys[entry.key.clone()];
            if !key.starts_with(&self.raw) {
                break;
            }
            if key.len() == self.raw.len() {
                continue;
            }
            if best.len() < 8 {
                best.push(index);
            } else if entry.weight > dict.entries[best[7]].weight {
                best[7] = index;
            } else {
                continue;
            }
            let mut at = best.len() - 1;
            while at > 0 && dict.entries[best[at - 1]].weight < dict.entries[best[at]].weight {
                best.swap(at - 1, at);
                at -= 1;
            }
        }
        for index in best {
            Self::add(dict, index, self.raw.len(), ui, &mut self.consumed);
        }
    }
    fn update(&mut self, ui: &mut Ui<'_>) {
        ui.clear();
        let mut preedit = self.acc.clone();
        preedit.extend_from_slice(&self.raw);
        ui.preedit(&preedit);
        if self.raw.is_empty() || self.dict.is_none() {
            return;
        }
        self.exact(self.raw.len(), ui);
        self.abbreviate(ui);
        if self.raw.len() >= 3 {
            self.complete(ui);
        }
        for len in (1..self.raw.len()).rev() {
            if ui.state.ncand as usize >= MAX_CANDIDATES {
                break;
            }
            self.exact(len, ui);
        }
        *ui.changed = true;
    }
    fn pick(&mut self, index: usize, out: &mut Output, ui: &mut Ui<'_>) {
        if index >= ui.state.ncand as usize {
            return;
        }
        append(&mut self.acc, 256, text(&ui.state.cand[index]));
        let used = self.consumed[index].min(self.raw.len());
        self.raw.drain(..used);
        if self.raw.is_empty() {
            out.append(&self.acc);
            self.clear(ui);
        } else {
            self.update(ui);
        }
    }
    pub(super) fn key(&mut self, key: &KeyInfo, out: &mut Output, ui: &mut Ui<'_>) -> bool {
        let empty = self.empty();
        let c = character(key);
        let name = text(&key.key);
        if c.is_ascii_lowercase() {
            append(&mut self.raw, 64, &[c]);
            self.update(ui);
            return true;
        }
        if empty {
            return false;
        }
        if let Some(digit) = ui.digit(key) {
            self.pick(digit, out, ui);
            return true;
        }
        match name {
            b" " => {
                if ui.state.ncand != 0 {
                    self.pick(ui.state.sel as usize, out, ui);
                } else {
                    self.commit_raw(out, ui);
                }
                return true;
            }
            b"Enter" => {
                self.commit_raw(out, ui);
                return true;
            }
            b"Escape" => {
                self.clear(ui);
                return true;
            }
            b"Backspace" => {
                if !self.raw.is_empty() {
                    self.raw.pop();
                } else {
                    drop_last(&mut self.acc);
                }
                if self.empty() {
                    self.clear(ui);
                } else {
                    self.update(ui);
                }
                return true;
            }
            b"ArrowDown" | b"ArrowRight" => {
                if ui.state.ncand != 0 {
                    ui.state.sel = (ui.state.sel + 1) % ui.state.ncand;
                    *ui.changed = true;
                }
                return true;
            }
            b"ArrowUp" | b"ArrowLeft" => {
                if ui.state.ncand != 0 {
                    ui.state.sel = (ui.state.sel + ui.state.ncand - 1) % ui.state.ncand;
                    *ui.changed = true;
                }
                return true;
            }
            _ => {}
        }
        if c == b'\'' {
            self.update(ui);
            return true;
        }
        if c == b'=' || name == b"PageDown" {
            if ui.state.sel as usize + PAGE < (ui.state.ncand as usize) {
                ui.state.sel = (ui.state.sel as usize / PAGE + 1) as i32 * PAGE as i32;
                *ui.changed = true;
            }
            return true;
        }
        if c == b'-' || name == b"PageUp" {
            if ui.state.sel as usize >= PAGE {
                ui.state.sel = (ui.state.sel as usize / PAGE - 1) as i32 * PAGE as i32;
                *ui.changed = true;
            }
            return true;
        }
        self.commit_raw(out, ui);
        false
    }
}

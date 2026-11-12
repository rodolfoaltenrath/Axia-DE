//! Visible video posters use the existing confined media worker and disk cache.
//! At most two requests run per window; changed entries invalidate old results.
use super::operations::Entry;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::PathBuf,
};

#[derive(Default)]
pub struct Cache {
    items: HashMap<PathBuf, (u64, Option<String>, u64)>,
    pending: HashMap<u64, (PathBuf, u64)>,
    sequence: u64,
    tick: u64,
}
fn version(e: &Entry) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&e.raw, e.size, e.mtime, e.stamp).hash(&mut h);
    h.finish()
}
impl Cache {
    pub fn get(&self, e: &Entry) -> Option<&str> {
        self.items
            .get(&e.raw)
            .filter(|item| item.0 == version(e))
            .and_then(|item| item.1.as_deref())
    }
    pub fn queue(&mut self, entries: &[Entry], effects: &mut Vec<Value>) {
        self.tick += 1;
        for e in entries.iter().filter(|e| {
            !e.dir
                && e.raw.is_absolute()
                && e.raw.to_str().is_some()
                && super::model::kind(&e.name) == "video"
        }) {
            let key = version(e);
            if let Some(item) = self.items.get_mut(&e.raw).filter(|item| item.0 == key) {
                item.2 = self.tick;
                continue;
            }
            if self.pending.len() >= 2 {
                break;
            }
            if self.items.len() >= 128 {
                if let Some(old) = self
                    .items
                    .iter()
                    .filter(|(p, _)| !self.pending.values().any(|(path, _)| path == *p))
                    .min_by_key(|(_, item)| item.2)
                    .map(|(p, _)| p.clone())
                {
                    self.items.remove(&old);
                }
            }
            self.sequence += 1;
            self.items.insert(e.raw.clone(), (key, None, self.tick));
            self.pending.insert(self.sequence, (e.raw.clone(), key));
            effects.push(json!({"kind":"videoThumbnail","path":e.raw,"generation":self.sequence}));
        }
    }
    pub fn complete(&mut self, event: &Value) {
        let Some((path, key)) = event["generation"]
            .as_u64()
            .and_then(|id| self.pending.remove(&id))
        else {
            return;
        };
        if let Some(item) = self.items.get_mut(&path).filter(|item| item.0 == key) {
            item.1 = event["result"]
                .as_str()
                .filter(|s| s.starts_with('/'))
                .map(str::to_owned);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{operations, tests::Home};
    use super::*;
    #[test]
    fn video_requests_are_bounded_cached_and_invalidated_on_change() {
        let home = Home::new();
        let mut entries: Vec<_> = (0..5)
            .map(|i| operations::entry(&home.file(&format!("clip-{i}.mp4"), "video")).unwrap())
            .collect();
        let mut cache = Cache::default();
        let mut effects = Vec::new();
        cache.queue(&entries, &mut effects);
        assert_eq!(effects.len(), 2);
        cache.queue(&entries, &mut effects);
        assert_eq!(effects.len(), 2);
        cache
            .complete(&json!({"generation":effects[0]["generation"],"result":"/cache/poster.png"}));
        assert_eq!(cache.get(&entries[0]), Some("/cache/poster.png"));
        cache.complete(&json!({"generation":effects[1]["generation"],"result":null}));
        cache.queue(&entries, &mut effects);
        assert_eq!(effects.len(), 4); // failed entry isn't retried on every repaint
        entries[0].stamp += 1;
        assert!(cache.get(&entries[0]).is_none());
        for e in effects[2..].to_vec() {
            cache.complete(&json!({"generation":e["generation"]}));
        }
        cache.queue(&entries[..1], &mut effects);
        let previous = effects.last().unwrap()["generation"].clone();
        entries[0].stamp += 1;
        cache.queue(&entries[..1], &mut effects);
        cache.complete(&json!({"generation":previous,"result":"/cache/stale.png"}));
        assert!(cache.get(&entries[0]).is_none());
        cache.complete(
            &json!({"generation":effects.last().unwrap()["generation"],"result":"/cache/new.png"}),
        );
        assert_eq!(cache.get(&entries[0]), Some("/cache/new.png"));
    }
}

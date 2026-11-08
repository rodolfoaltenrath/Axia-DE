//! Small first-page previews: Rust schedules at most two confined document jobs.
//! 32 retained 128×176 RGBA images use at most 2.75 MiB per window.
use super::operations::Entry;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::PathBuf,
};
#[derive(Default)]
pub struct Cache {
    items: HashMap<PathBuf, Item>,
    pending: HashMap<u64, (PathBuf, u64)>,
    tick: u64,
    sequence: u64,
}
struct Item {
    version: u64,
    image: Option<Value>,
    touched: u64,
}
fn version(e: &Entry) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    e.raw.hash(&mut h);
    e.size.hash(&mut h);
    e.mtime.hash(&mut h);
    e.stamp.hash(&mut h);
    h.finish()
}
fn release(image: Option<Value>, effects: &mut Vec<Value>) {
    if let Some(image) = image {
        effects.push(json!({"kind":"closePreview","image":image}));
    }
}
impl Cache {
    pub fn get(&self, e: &Entry) -> Option<&Value> {
        self.items
            .get(&e.raw)
            .filter(|v| v.version == version(e))
            .and_then(|v| v.image.as_ref())
    }
    pub fn queue(&mut self, entries: &[Entry], effects: &mut Vec<Value>) {
        self.tick += 1;
        for e in entries
            .iter()
            .filter(|e| {
                !e.dir
                    && e.raw.is_absolute()
                    && e.raw.to_str().is_some()
                    && super::model::kind(&e.name) == "pdf"
            })
            .take(32)
        {
            let key = version(e);
            if let Some(item) = self.items.get_mut(&e.raw) {
                if item.version == key {
                    item.touched = self.tick;
                    continue;
                }
            }
            if self.pending.len() >= 2 {
                break;
            }
            if let Some(old) = self.items.remove(&e.raw) {
                release(old.image, effects);
            }
            if self.items.len() >= 32 {
                let oldest = self
                    .items
                    .iter()
                    .filter(|(p, _)| !self.pending.values().any(|(q, _)| q == *p))
                    .min_by_key(|(_, v)| v.touched)
                    .map(|(p, _)| p.clone());
                if let Some(path) = oldest {
                    release(self.items.remove(&path).and_then(|v| v.image), effects);
                } else {
                    break;
                }
            }
            self.sequence += 1;
            self.pending.insert(self.sequence, (e.raw.clone(), key));
            self.items.insert(
                e.raw.clone(),
                Item {
                    version: key,
                    image: None,
                    touched: self.tick,
                },
            );
            effects.push(json!({"kind":"pdfThumbnail","path":e.raw,"generation":self.sequence}));
        }
    }
    pub fn complete(&mut self, event: &Value, effects: &mut Vec<Value>) {
        let image = event["result"]
            .as_object()
            .filter(|v| v.get("id").is_some_and(|v| v.as_u64().is_some()))
            .map(|_| event["result"].clone());
        if let Some((path, key)) = event["generation"]
            .as_u64()
            .and_then(|id| self.pending.remove(&id))
        {
            if let Some(item) = self.items.get_mut(&path).filter(|v| v.version == key) {
                item.image = image;
                return;
            }
        }
        release(image, effects);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(n: usize) -> Entry {
        Entry {
            name: format!("page-{n}.pdf"),
            path: format!("/home/test/page-{n}.pdf"),
            raw: PathBuf::from(format!("/home/test/page-{n}.pdf")),
            dir: false,
            link: false,
            size: 100,
            mtime: 1,
            stamp: 1,
            mode: 0,
            writable: true,
            original: None,
        }
    }
    #[test]
    fn jobs_are_bounded_failures_do_not_retry_and_stale_images_are_released() {
        let mut c = Cache::default();
        let mut effects = vec![];
        let entries: Vec<_> = (0..100).map(entry).collect();
        c.queue(&entries, &mut effects);
        assert_eq!(effects.len(), 2);
        c.complete(&json!({"generation":1,"error":"invalid"}), &mut effects);
        c.complete(&json!({"generation":2,"result":{"id":22}}), &mut effects);
        effects.clear();
        c.queue(&entries, &mut effects);
        assert!(effects.iter().all(|v| v["path"] != entries[0].path));
        assert!(c.get(&entries[1]).is_some());
        assert_eq!(c.pending.len(), 2);
        c.complete(&json!({"generation":999,"result":{"id":99}}), &mut effects);
        assert!(effects
            .iter()
            .any(|v| v["kind"] == "closePreview" && v["image"]["id"] == 99));
    }
    #[test]
    fn cache_eviction_releases_images_and_changed_files_are_regenerated() {
        let mut c = Cache::default();
        let mut effects = vec![];
        for n in 0..40 {
            c.queue(&[entry(n)], &mut effects);
            let id = c.sequence;
            c.complete(&json!({"generation":id,"result":{"id":n+1}}), &mut effects);
            assert!(c.items.len() <= 32);
        }
        assert!(effects.iter().any(|v| v["kind"] == "closePreview"));
        let mut changed = entry(39);
        changed.size += 1;
        assert!(c.get(&changed).is_none());
        effects.clear();
        c.queue(&[changed], &mut effects);
        assert!(effects.iter().any(|v| v["kind"] == "pdfThumbnail"));
    }
}

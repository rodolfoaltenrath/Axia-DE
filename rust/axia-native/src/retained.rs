//! One revisioned subtree per semantic region. The producer retains its full
//! specification for remount/context recovery; ordinary syncs return a small
//! marker instead of rebuilding or cloning all descendant widget values.
use serde_json::{json, Value};
#[derive(Default)]
pub struct RetainedTree {
    signature: Option<Value>,
    revision: u64,
    tree: Value,
}
impl RetainedTree {
    pub fn render(&mut self, key: &str, signature: Value, build: impl FnOnce() -> Value) -> Value {
        if self.signature.as_ref() != Some(&signature) || self.tree["key"] != key {
            self.tree = build();
            self.signature = Some(signature);
            self.revision = self
                .revision
                .checked_add(1)
                .expect("retained tree revision overflow");
            self.tree["key"] = json!(key);
            self.tree["revision"] = json!(self.revision);
        }
        json!({"kind":self.tree["kind"],"key":key,"revision":self.revision,"retained":true})
    }
    pub fn clear(&mut self) {
        self.signature = None;
        self.tree = Value::Null;
    }
    pub fn tree(&self, key: &str) -> Option<&Value> {
        (self.tree["key"] == key).then_some(&self.tree)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_revisions_do_not_build_and_full_specs_remain_available() {
        let mut cache = RetainedTree::default();
        let a = cache.render(
            "list",
            json!([1, "context"]),
            || json!({"kind":"view","children":[{"kind":"text","label":"hello"}]}),
        );
        let b = cache.render("list", json!([1, "context"]), || {
            panic!("unchanged subtree rebuilt")
        });
        assert_eq!(a, b);
        assert!(a["children"].is_null());
        assert_eq!(cache.tree("list").unwrap()["children"][0]["label"], "hello");
        let c = cache.render("list", json!([2, "context"]), || json!({"kind":"view"}));
        assert_ne!(a["revision"], c["revision"]);
        assert!(cache.tree("other").is_none());
    }
}

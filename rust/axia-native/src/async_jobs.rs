//! Transport state for compiled apps using runtime/native_async.js. Tags belong
//! to each app; only JSON transport, fragmentation and cancellation live here.
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
struct Request<T> {
    tag: T,
    method: &'static str,
    args: Value,
    encoded: String,
    cancelled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_covers_starting_jobs_and_never_redirects_tokens() {
        let mut jobs = Jobs::default();
        jobs.enqueue(1, "test", json!([]));
        assert!(jobs.pump(0).is_none());
        jobs.bind(7);
        assert_eq!(jobs.pump(0).unwrap()["args"][0], 7);
        jobs.cancel(|tag| *tag == 1);
        assert!(jobs.complete(&json!(99), 0).is_empty());
        let discard = jobs.pump(100).unwrap();
        assert_eq!(discard["args"][1], json!([]));
        assert_eq!(discard["args"][2], json!([99]));
        jobs.complete(&json!({"value":[]}), 100);
        assert!(!jobs.busy());
    }
    #[test]
    fn chunks_and_errors_are_independent_and_polling_yields_between_reads() {
        let mut jobs = Jobs::default();
        jobs.bind(1);
        jobs.enqueue(1, "test", json!([]));
        jobs.enqueue(2, "test", json!([]));
        jobs.pump(0);
        jobs.complete(&json!(10), 0);
        jobs.pump(0);
        jobs.complete(&json!(11), 0);
        assert!(jobs.pump(10).is_none());
        jobs.pump(100);
        let done=jobs.complete(&json!({"value":[{"token":10,"part":"{\"hello\":","done":false},{"token":11,"error":"denied"}]}),100);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].tag, 2);
        assert!(done[0].result.is_err());
        jobs.pump(200);
        let done = jobs.complete(
            &json!({"value":[{"token":10,"part":"\"世界\"}","done":true}]}),
            200,
        );
        assert_eq!(done[0].result.as_ref().unwrap()["hello"], "世界");
        assert!(!jobs.busy());
    }
}
enum Step<T> {
    Start(Request<T>),
    Take,
}
pub struct Completed<T> {
    pub tag: T,
    pub result: Result<Value, String>,
}
pub struct Jobs<T> {
    owner: i64,
    queue: VecDeque<Request<T>>,
    pending: BTreeMap<u64, Request<T>>,
    step: Option<Step<T>>,
    discard: Vec<u64>,
    poll_at: i64,
}
impl<T> Default for Jobs<T> {
    fn default() -> Self {
        Self {
            owner: 0,
            queue: VecDeque::new(),
            pending: BTreeMap::new(),
            step: None,
            discard: vec![],
            poll_at: 0,
        }
    }
}
impl<T> Jobs<T> {
    pub fn bind(&mut self, owner: i64) {
        self.owner = owner;
    }
    pub fn busy(&self) -> bool {
        !self.queue.is_empty()
            || !self.pending.is_empty()
            || self.step.is_some()
            || !self.discard.is_empty()
    }
    pub fn enqueue(&mut self, tag: T, method: &'static str, args: Value) {
        self.queue.push_back(Request {
            tag,
            method,
            args,
            encoded: String::new(),
            cancelled: false,
        });
    }
    pub fn cancel(&mut self, mut predicate: impl FnMut(&T) -> bool) {
        self.queue.retain(|r| !predicate(&r.tag));
        if let Some(Step::Start(r)) = &mut self.step {
            if predicate(&r.tag) {
                r.cancelled = true;
            }
        }
        let obsolete: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(id, r)| predicate(&r.tag).then_some(*id))
            .collect();
        for id in obsolete {
            self.pending.remove(&id);
            self.discard.push(id);
        }
    }
    pub fn complete(&mut self, result: &Value, now: i64) -> Vec<Completed<T>> {
        let mut finished = vec![];
        match self.step.take() {
            Some(Step::Start(r)) => {
                if let Some(token) = result.as_u64() {
                    if r.cancelled {
                        self.discard.push(token);
                    } else {
                        self.pending.insert(token, r);
                    }
                } else if !r.cancelled {
                    finished.push(Completed {
                        tag: r.tag,
                        result: Err(result["error"]
                            .as_str()
                            .unwrap_or("app.invalidRequest")
                            .into()),
                    });
                }
            }
            Some(Step::Take) => {
                for packet in result["value"].as_array().into_iter().flatten() {
                    let Some(token) = packet["token"].as_u64() else {
                        continue;
                    };
                    if let Some(part) = packet["part"].as_str() {
                        if let Some(r) = self.pending.get_mut(&token) {
                            r.encoded.push_str(part);
                        }
                        if packet["done"] != true {
                            continue;
                        }
                    }
                    if let Some(r) = self.pending.remove(&token) {
                        let value = if let Some(error) = packet["error"].as_str() {
                            Err(error.into())
                        } else if packet["part"].is_string() {
                            serde_json::from_str(&r.encoded)
                                .map_err(|_| "app.invalidRequest".into())
                        } else {
                            Ok(packet["value"].clone())
                        };
                        finished.push(Completed {
                            tag: r.tag,
                            result: value,
                        });
                    }
                }
            }
            None => {}
        }
        self.poll_at = now + 50;
        finished
    }
    pub fn pump(&mut self, now: i64) -> Option<Value> {
        if self.owner <= 0 || self.step.is_some() {
            return None;
        }
        if let Some(r) = self.queue.pop_front() {
            let effect = json!({"kind":"nativeCall","service":"nativeAsync","method":"start","args":[self.owner,r.method,r.args]});
            self.step = Some(Step::Start(r));
            return Some(effect);
        }
        if now < self.poll_at || self.pending.is_empty() && self.discard.is_empty() {
            return None;
        }
        self.poll_at = now + 50;
        self.step = Some(Step::Take);
        Some(
            json!({"kind":"nativeRead","service":"nativeAsync","method":"take","args":[self.owner,self.pending.keys().collect::<Vec<_>>(),std::mem::take(&mut self.discard)]}),
        )
    }
}

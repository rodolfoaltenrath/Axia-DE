//! Output naming and reservations. The host executes closed directory/stat
//! requests; no filename, collision loop or destination policy lives in it.
use axia_native::i18n::I18n;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
enum Stage {
    Dirs,
    Pictures,
    Folder,
    Stat,
    Ready,
}
struct Job {
    kind: String,
    stage: Stage,
    step: u32,
    folder_name: String,
    pictures: String,
    folder: String,
    base: String,
    ext: &'static str,
    attempt: u32,
    wall: i64,
    path: String,
    clipboard: bool,
    mic: bool,
    app_name: String,
}
#[derive(Default)]
pub struct Outputs {
    next: u64,
    jobs: BTreeMap<u64, Job>,
    reserved: BTreeSet<String>,
}
fn component(value: &str) -> String {
    value.replace(['/', '\\', '\0'], "-")
}
pub fn stamp(ms: i64) -> String {
    let tm = axia_native::clock::local(ms);
    format!(
        "{:04}-{:02}-{:02} {:02}.{:02}.{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}
impl Job {
    fn reply(&self, id: u64) -> Value {
        let effect = match self.stage {
            Stage::Dirs => json!({"op":"dirs"}),
            Stage::Pictures => json!({"op":"mkdir","path":self.pictures}),
            Stage::Folder => json!({"op":"mkdir","path":self.folder}),
            Stage::Stat => json!({"op":"stat","path":self.path}),
            Stage::Ready => {
                return json!({"id":id,"target":{"path":self.path,"clipboard":self.clipboard,"mic":self.mic,"appName":self.app_name}})
            }
        };
        json!({"id":id,"step":self.step,"effect":effect})
    }
    fn candidate(&self, id: u64) -> String {
        let suffix = if self.attempt == 1 {
            String::new()
        } else if self.attempt < 1000 {
            format!(" ({})", self.attempt)
        } else {
            format!(
                " {}{}",
                self.wall,
                if self.attempt == 1000 {
                    String::new()
                } else {
                    format!("-{id}")
                }
            )
        };
        format!("{}/{}{suffix}.{}", self.folder, self.base, self.ext)
    }
}
impl Outputs {
    pub fn begin(&mut self, input: &Value, prefs: &Value, i18n: &mut I18n, wall: i64) -> Value {
        let Some(kind) = input["kind"]
            .as_str()
            .filter(|s| ["image", "record", "annotated"].contains(s))
        else {
            return json!({"error":"app.invalidRequest"});
        };
        if self.jobs.len() >= 128 {
            return json!({"error":"app.invalidRequest"});
        }
        let Some(id) = self
            .next
            .checked_add(1)
            .filter(|v| *v <= 9_007_199_254_740_991)
        else {
            return json!({"error":"app.invalidRequest"});
        };
        self.next = id;
        let date = stamp(wall);
        let mut params = serde_json::Map::new();
        params.insert("date".into(), json!(date));
        let mut base = i18n.t(
            if kind == "record" {
                "screenshots.recName"
            } else {
                "screenshots.fileName"
            },
            &params,
        );
        if kind == "annotated" {
            if let Some(name) = input["source"]
                .as_str()
                .and_then(|s| s.rsplit('/').next())
                .filter(|s| !s.is_empty())
            {
                base = if name
                    .get(name.len().saturating_sub(4)..)
                    .is_some_and(|s| s.eq_ignore_ascii_case(".png"))
                {
                    name[..name.len() - 4].into()
                } else {
                    name.into()
                };
            }
            base = format!(
                "{base} ({})",
                i18n.t("screenshots.annotated", &Default::default())
            );
        }
        let folder = component(&i18n.t("screenshots.folder", &Default::default()));
        let folder = if folder.is_empty() || [".", ".."].contains(&folder.as_str()) {
            "Screenshots".into()
        } else {
            folder
        };
        let clipboard = kind == "image" && prefs["clipboard"] == true;
        let job = Job {
            kind: kind.into(),
            stage: if clipboard { Stage::Ready } else { Stage::Dirs },
            step: 0,
            folder_name: folder,
            pictures: String::new(),
            folder: String::new(),
            base: component(&base),
            ext: if kind == "record" { "mp4" } else { "png" },
            attempt: 1,
            wall,
            path: String::new(),
            clipboard,
            mic: prefs["mic"] == true,
            app_name: i18n.t("apps.screenshots", &Default::default()),
        };
        let reply = job.reply(id);
        self.jobs.insert(id, job);
        reply
    }
    pub fn next(&mut self, input: &Value) -> Value {
        let Some(id) = input["id"].as_u64() else {
            return json!({"error":"app.invalidRequest"});
        };
        let Some(job) = self.jobs.get_mut(&id).filter(|j| {
            Some(u64::from(j.step)) == input["step"].as_u64() && !matches!(j.stage, Stage::Ready)
        }) else {
            return json!({"error":"app.invalidRequest"});
        };
        let result = &input["result"];
        match job.stage {
            Stage::Dirs => {
                let Some(pictures) = result["pictures"]
                    .as_str()
                    .filter(|s| s.starts_with('/') && !s.contains('\0'))
                else {
                    self.release(id);
                    return json!({"id":id,"error":"app.invalidRequest"});
                };
                job.pictures = pictures.trim_end_matches('/').into();
                job.folder = format!("{}/{}", job.pictures, job.folder_name);
                job.stage = Stage::Pictures;
            }
            // Creating an existing folder is harmless; the filesystem service
            // remains authoritative for access and write permission failures.
            Stage::Pictures => job.stage = Stage::Folder,
            Stage::Folder => job.stage = Stage::Stat,
            Stage::Stat => {
                if result["exists"] == true || self.reserved.contains(&job.path) {
                    job.attempt += 1;
                } else {
                    self.reserved.insert(job.path.clone());
                    job.stage = Stage::Ready;
                }
            }
            Stage::Ready => unreachable!(),
        }
        if matches!(job.stage, Stage::Stat) {
            loop {
                job.path = job.candidate(id);
                if !self.reserved.contains(&job.path) {
                    break;
                }
                job.attempt += 1;
                if job.attempt > 1001 {
                    self.release(id);
                    return json!({"id":id,"error":"fs.exists"});
                }
            }
            // Preserve the historical timestamp fallback after 999 numbered
            // candidates. Media output still refuses to replace existing files.
            if job.attempt >= 1000 {
                self.reserved.insert(job.path.clone());
                job.stage = Stage::Ready;
            }
        }
        job.step += 1;
        job.reply(id)
    }
    pub fn release(&mut self, id: u64) {
        if let Some(job) = self.jobs.remove(&id) {
            if matches!(job.stage, Stage::Ready) {
                self.reserved.remove(&job.path);
            }
        }
    }
    pub fn complete(&mut self, input: &Value) -> Value {
        let Some(id) = input["id"].as_u64().filter(|id| {
            self.jobs
                .get(id)
                .is_some_and(|j| matches!(j.stage, Stage::Ready))
        }) else {
            return json!({"error":"app.invalidRequest"});
        };
        let job = self.jobs.remove(&id).unwrap();
        self.reserved.remove(&job.path);
        let ok = input["result"]["ok"] == true;
        let path = if ok && !job.clipboard {
            Some(
                input["result"]["path"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&job.path),
            )
        } else {
            None
        };
        json!({"kind":job.kind,"ok":ok,"path":path,"clipboard":job.clipboard})
    }
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

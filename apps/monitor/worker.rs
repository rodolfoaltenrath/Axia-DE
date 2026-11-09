//! One sleeping sampler for all Monitor windows and the shell's disk/history subscriptions.
//! Slow procfs/sysfs walks and journal writes never hold the UI snapshot mutex.
use super::{connections, geo::Database, history::History, metrics, processes, read, sensors};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, OnceLock},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Want {
    pub processes: bool,
    pub connections: bool,
    pub geo: bool,
    pub voltage: bool,
    pub cooling: bool,
    pub detail: Option<(i32, u64)>,
}
#[derive(Clone, Default)]
pub struct Snapshot {
    pub seq: u64,
    pub samples: VecDeque<Value>,
    pub processes: Value,
    pub connections: Value,
    pub sensors: Value,
    pub fan: Value,
    pub info: Value,
    pub details: BTreeMap<(i32, u64), Value>,
    pub history: Value,
    pub result: Value,
    pub totals: Value,
    pub maps: BTreeMap<u64, Arc<super::charts::Raster>>,
}
pub enum Job {
    Signal(i32, u64, String),
    FanForce(bool),
    Clear,
    Query(Value),
    Counts(Value),
    Totals,
    Map(u64, super::charts::MapView),
    Preferences,
}
struct State {
    snapshot: Arc<Snapshot>,
    clients: BTreeMap<u64, Want>,
    next: u64,
    watch: bool,
    history: bool,
    allowed: bool,
    jobs: VecDeque<Job>,
}
pub struct Service {
    state: Mutex<State>,
    wake: Condvar,
}
pub struct Lease {
    pub service: Arc<Service>,
    pub id: u64,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.service
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clients
            .remove(&self.id);
        self.service.wake.notify_one();
    }
}
pub fn wall() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn history_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|s| PathBuf::from(s).is_absolute())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|s| PathBuf::from(s).join(".local/state")))
        .map(|p| p.join("axia/net-history.bin"))
        .unwrap_or_default()
}
impl Service {
    pub fn shared() -> Arc<Self> {
        static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();
        SERVICE
            .get_or_init(|| Self::new(PathBuf::from("/"), history_path()))
            .clone()
    }
    pub fn new(root: PathBuf, path: PathBuf) -> Arc<Self> {
        let service = Arc::new(Self {
            state: Mutex::new(State {
                snapshot: Arc::new(Snapshot::default()),
                clients: BTreeMap::new(),
                next: 1,
                watch: false,
                history: false,
                allowed: false,
                jobs: VecDeque::new(),
            }),
            wake: Condvar::new(),
        });
        let copy = service.clone();
        thread::Builder::new()
            .name("axia-monitor".into())
            .spawn(move || copy.run(root, path))
            .expect("monitor worker");
        service
    }
    pub fn lease(self: &Arc<Self>) -> Lease {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let id = state.next;
        state.next = state.next.wrapping_add(1).max(1);
        state.clients.insert(id, Want::default());
        drop(state);
        self.wake.notify_one();
        Lease {
            service: self.clone(),
            id,
        }
    }
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .snapshot
            .clone()
    }
    pub fn update(&self, lease: &Lease, want: Want) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let changed = state.clients.get_mut(&lease.id).is_some_and(|w| {
            if *w == want {
                return false;
            }
            *w = want;
            true
        });
        drop(state);
        if changed {
            self.wake.notify_one();
        }
    }
    pub fn legacy(&self, on: bool, want: Option<Want>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if on {
            if let Some(w) = want {
                state.clients.insert(0, w);
            } else {
                state.clients.entry(0).or_default();
            }
        } else {
            state.clients.remove(&0);
        }
        drop(state);
        self.wake.notify_one();
    }
    pub fn watch(&self, on: bool) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).watch = on;
        self.wake.notify_one();
    }
    pub fn history(&self, on: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let changed = state.history != on;
        state.history = on;
        drop(state);
        if changed {
            self.wake.notify_one();
        }
    }
    pub fn allowed(&self, on: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.allowed = on;
        if on {
            state.jobs.push_back(Job::Preferences);
        }
        drop(state);
        self.wake.notify_one();
    }
    pub fn recording(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.allowed && state.history
    }
    pub fn job(&self, job: Job) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.jobs.len() < 128 {
            state.jobs.push_back(job);
        }
        drop(state);
        self.wake.notify_one();
    }
    fn run(&self, root: PathBuf, path: PathBuf) {
        let start = Instant::now();
        let mut sampler = metrics::Sampler::default();
        let mut proc = processes::Collector::default();
        let mut snapshot = Snapshot {
            info: metrics::info(&root),
            ..Default::default()
        };
        let mut history = History::new(path);
        let mut previous_net = None;
        let mut previous_counts: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        let mut last_sample = Instant::now() - Duration::from_secs(30);
        let mut last_second = Instant::now() - Duration::from_secs(5);
        let mut last_sensor = Instant::now() - Duration::from_secs(5);
        let mut last_history = Instant::now() - Duration::from_secs(5);
        let mut geo = None;
        let mut geo_tried = false;
        loop {
            let (wants, watch, enabled, jobs) = {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                while state.clients.is_empty()
                    && !state.watch
                    && !(state.allowed && state.history)
                    && state.jobs.is_empty()
                {
                    state = self.wake.wait(state).unwrap_or_else(|e| e.into_inner());
                    previous_net = None;
                    sampler = metrics::Sampler::default();
                    last_sample = Instant::now() - Duration::from_secs(30);
                }
                let jobs = state.jobs.drain(..).collect::<Vec<_>>();
                snapshot.maps.retain(|id, _| state.clients.contains_key(id));
                (
                    state.clients.values().cloned().collect::<Vec<_>>(),
                    state.watch,
                    state.allowed && state.history,
                    jobs,
                )
            };
            let now = wall();
            if history.enabled && !enabled {
                let _ = history.save(now / 1000 + 60);
            }
            history.enabled = enabled;
            let active = !wants.is_empty();
            let collect_procs = wants.iter().any(|w| w.processes);
            let collect_conns = wants.iter().any(|w| w.connections);
            let locate = wants.iter().any(|w| w.connections && w.geo);
            for job in jobs {
                match job {
                    Job::FanForce(on) => {
                        snapshot.result = json!({"error":super::fan::force(on).err()});
                        snapshot.fan = super::fan::status();
                    }
                    Job::Preferences => {
                        let path = std::env::var_os("XDG_CONFIG_HOME")
                            .map(PathBuf::from)
                            .or_else(|| {
                                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
                            })
                            .map(|p| p.join("axia/settings.json"));
                        let prefs: Value = path
                            .and_then(|p| serde_json::from_str(&read(&p, 1 << 20)).ok())
                            .unwrap_or(Value::Null);
                        self.state.lock().unwrap_or_else(|e| e.into_inner()).history =
                            prefs["monitor"]["history"] != false;
                    }
                    Job::Map(id, q) => {
                        static LAND: OnceLock<Vec<Vec<(f64, f64)>>> = OnceLock::new();
                        let rings = LAND.get_or_init(|| {
                            super::geo::outlines(include_bytes!("../../assets/monitor/land.bin"))
                                .unwrap_or_default()
                        });
                        let raster = super::charts::world(
                            q.width,
                            280,
                            rings,
                            &q.places,
                            q.home,
                            q.lon,
                            q.lat,
                            q.zoom,
                            q.generation,
                        );
                        snapshot.maps.insert(id, Arc::new(raster));
                    }
                    Job::Totals => {
                        let total = metrics::net(&read(&root.join("proc/net/dev"), 65536))
                            .into_iter()
                            .filter(|(n, _)| n != "lo")
                            .fold((0u64, 0u64), |(rx, tx), (_, v)| {
                                (rx.saturating_add(v.0), tx.saturating_add(v.1))
                            });
                        snapshot.totals = json!({"rx":total.0,"tx":total.1,"t":now});
                    }
                    Job::Signal(pid, st, kind) => {
                        snapshot.result =
                            json!({"pid":pid,"error":processes::signal(pid,st,&kind).err()});
                    }
                    Job::Clear => {
                        snapshot.result = json!({"cleared":history.clear()});
                        snapshot.history = Value::Null;
                    }
                    Job::Query(args) => {
                        snapshot.history = history.query(
                            args["from"].as_u64().unwrap_or(now / 1000 - 3600),
                            args["step"].as_u64().unwrap_or(60),
                            args["n"].as_u64().unwrap_or(60).min(1500) as usize,
                            args["app"].as_str().unwrap_or(""),
                            now / 1000,
                        );
                    }
                    Job::Counts(counts) => {
                        if let Some(map) = counts.as_object() {
                            for (app, v) in map.iter().take(1024) {
                                let current =
                                    (v[2].as_u64().unwrap_or(0), v[3].as_u64().unwrap_or(0));
                                if let Some(old) = previous_counts.insert(app.clone(), current) {
                                    history.add(
                                        now / 1000,
                                        app,
                                        if current.0 >= old.0 {
                                            current.0 - old.0
                                        } else {
                                            current.0
                                        },
                                        if current.1 >= old.1 {
                                            current.1 - old.1
                                        } else {
                                            current.1
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if last_sensor.elapsed() >= Duration::from_secs(if active { 2 } else { 30 }) {
                snapshot.sensors = sensors::collect(&root);
                if root == std::path::Path::new("/") && wants.iter().any(|w| w.cooling) {
                    snapshot.fan = super::fan::status();
                }
                snapshot.sensors["t"] = json!(now);
                last_sensor = Instant::now();
            }
            let sample_interval = Duration::from_millis(if active { 250 } else { 30_000 });
            if (active || watch) && last_sample.elapsed() >= sample_interval {
                last_sample = Instant::now();
                let sample = sampler.sample(
                    &root,
                    start.elapsed().as_secs_f64() + 0.001,
                    now,
                    &snapshot.sensors,
                );
                if active {
                    snapshot.samples.push_back(sample);
                    while snapshot.samples.len() > 240 {
                        snapshot.samples.pop_front();
                    }
                } else {
                    snapshot.samples.clear();
                    snapshot.samples.push_back(sample);
                }
            }
            if last_second.elapsed() >= Duration::from_secs(1) {
                if collect_procs {
                    snapshot.processes = proc.scan(&root, start.elapsed().as_secs_f64());
                } else {
                    snapshot.processes = Value::Null;
                }
                if locate && !geo_tried {
                    geo_tried = true;
                    geo = Database::parse(
                        include_bytes!("../../assets/monitor/geoip-country.mmdb").to_vec(),
                    );
                }
                if collect_conns {
                    snapshot.connections =
                        connections::collect(&root, if locate { geo.as_ref() } else { None });
                    snapshot.connections["t"] = json!(now);
                } else {
                    snapshot.connections = Value::Null;
                    geo = None;
                    geo_tried = false;
                }
                snapshot.details.clear();
                for (pid, st) in wants.iter().filter_map(|w| w.detail).take(16) {
                    snapshot
                        .details
                        .insert((pid, st), processes::detail(&root, pid, st));
                }
                last_second = Instant::now();
            }
            if enabled && last_history.elapsed() >= Duration::from_secs(5) {
                let nets = metrics::net(&read(&root.join("proc/net/dev"), 65536));
                let total = nets
                    .iter()
                    .filter(|(n, _)| n.as_str() != "lo")
                    .fold((0u64, 0u64), |(rx, tx), (_, v)| {
                        (rx.saturating_add(v.0), tx.saturating_add(v.1))
                    });
                if let Some(old) = previous_net {
                    let old: (u64, u64) = old;
                    history.add(
                        now / 1000,
                        "",
                        total.0.saturating_sub(old.0),
                        total.1.saturating_sub(old.1),
                    );
                }
                previous_net = Some(total);
                let _ = history.save(now / 1000);
                last_history = Instant::now();
            } else if !enabled {
                previous_net = None;
            }
            snapshot.seq = snapshot.seq.wrapping_add(1);
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.snapshot = Arc::new(snapshot.clone());
            let delay = if active {
                Duration::from_millis(250).saturating_sub(last_sample.elapsed())
            } else if enabled {
                Duration::from_secs(5)
            } else {
                Duration::from_secs(30)
            };
            let _ = self
                .wake
                .wait_timeout(state, delay)
                .unwrap_or_else(|e| e.into_inner());
        }
    }
}

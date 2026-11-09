//! CPU, network, block I/O and memory sampling. Rates use monotonic elapsed time.
use super::{memory, read, sensors};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};
#[derive(Default)]
pub struct Sampler {
    cpu: Vec<(u64, u64)>,
    net: BTreeMap<String, (u64, u64)>,
    disk: BTreeMap<String, (u64, u64, u64)>,
    disk_active: BTreeMap<String, f64>,
    disk_temp: BTreeMap<String, (f64, Value, u64)>,
    at: f64,
    pub sequence: u64,
}
pub fn cpu(text: &str) -> Vec<(u64, u64)> {
    text.lines()
        .take_while(|l| l.starts_with("cpu"))
        .take(257)
        .map(|line| {
            let v: Vec<u64> = line
                .split_whitespace()
                .skip(1)
                .map(|s| s.parse().unwrap_or(0))
                .collect();
            let total = v.iter().take(8).fold(0u64, |n, v| n.saturating_add(*v));
            let idle = v
                .get(3)
                .copied()
                .unwrap_or(0)
                .saturating_add(v.get(4).copied().unwrap_or(0));
            (total, total.saturating_sub(idle))
        })
        .collect()
}
pub fn net(text: &str) -> BTreeMap<String, (u64, u64)> {
    text.lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(':')?;
            let v: Vec<_> = rest.split_whitespace().collect();
            Some((
                name.trim().to_owned(),
                (v.first()?.parse().ok()?, v.get(8)?.parse().ok()?),
            ))
        })
        .take(256)
        .collect()
}
fn rate(now: u64, old: Option<u64>, dt: f64) -> f64 {
    old.filter(|_| dt > 0.)
        .map(|old| now.saturating_sub(old) as f64 / dt)
        .unwrap_or(0.)
}
impl Sampler {
    pub fn sample(&mut self, root: &Path, at: f64, wall: u64, sensors: &Value) -> Value {
        let dt = if self.at > 0. { at - self.at } else { 0. };
        let current = cpu(&read(&root.join("proc/stat"), 65536));
        let usages: Vec<_> = current
            .iter()
            .enumerate()
            .map(|(i, (total, busy))| {
                self.cpu
                    .get(i)
                    .map(|(t, b)| {
                        if total > t {
                            (busy.saturating_sub(*b) as f64 / total.saturating_sub(*t) as f64)
                                .clamp(0., 1.)
                        } else {
                            0.
                        }
                    })
                    .unwrap_or(0.)
            })
            .collect();
        self.cpu = current;
        let nets = net(&read(&root.join("proc/net/dev"), 65536));
        let mut rx = 0.;
        let mut tx = 0.;
        let mut rxt = 0u64;
        let mut txt = 0u64;
        let mut ifs = Vec::new();
        for (name, (r, t)) in &nets {
            let rr = rate(*r, self.net.get(name).map(|v| v.0), dt);
            let tr = rate(*t, self.net.get(name).map(|v| v.1), dt);
            if name != "lo" {
                rx += rr;
                tx += tr;
                rxt = rxt.saturating_add(*r);
                txt = txt.saturating_add(*t)
            }
            ifs.push(json!({"name":name,"rx":rr,"tx":tr,"rxTotal":r,"txTotal":t}));
        }
        self.net = nets;
        let mut disks = Vec::new();
        let mut next = BTreeMap::new();
        let mut rd = 0.;
        let mut wr = 0.;
        for line in read(&root.join("proc/diskstats"), 65536).lines() {
            let v: Vec<_> = line.split_whitespace().collect();
            let Some(name) = v.get(2).copied() else {
                continue;
            };
            if name.starts_with("loop")
                || name.starts_with("ram")
                || !root.join("sys/block").join(name).exists()
            {
                continue;
            }
            let field = |i: usize| v.get(i).and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
            let r = field(5).saturating_mul(512);
            let w = field(9).saturating_mul(512);
            let ios = field(3).saturating_add(field(7));
            let old = self.disk.get(name);
            let rr = rate(r, old.map(|o| o.0), dt);
            let tr = rate(w, old.map(|o| o.1), dt);
            rd += rr;
            wr += tr;
            let path = root.join("sys/block").join(name);
            let rota = read(&path.join("queue/rotational"), 32).trim() == "1";
            if old.is_some_and(|o| o.2 != ios) {
                self.disk_active.insert(name.into(), at);
            }
            let awake = !rota
                || self
                    .disk_active
                    .get(name)
                    .is_some_and(|last| at - last < 60.);
            let mut disk = json!({"name":name,"model":read(&path.join("device/model"),256).trim(),"size":read(&path.join("size"),64).trim().parse::<u64>().unwrap_or(0).saturating_mul(512),"rotational":rota,"rota":rota,"awake":awake,"read":rr,"write":tr,"health":"unknown"});
            if self.disk_temp.get(name).is_none_or(|old| at - old.0 >= 3.) {
                self.disk_temp.insert(
                    name.into(),
                    (at, sensors::disk_temperature(root, name, awake), wall),
                );
            }
            if let Some((_, extra, t)) = self.disk_temp.get(name) {
                if let Some(extra) = extra.as_object() {
                    disk.as_object_mut().unwrap().extend(extra.clone());
                }
                disk["tempT"] = json!(t);
            }
            disks.push(disk);
            next.insert(name.into(), (r, w, ios));
            if disks.len() >= 128 {
                break;
            }
        }
        self.disk = next;
        let load = sensors::load(&read(&root.join("proc/loadavg"), 256));
        let mem = memory::parse(&read(&root.join("proc/meminfo"), 16384));
        self.at = at;
        self.sequence += 1;
        json!({"t":wall,"seq":self.sequence,"dt":dt*1000.,"cpu":usages.first().copied().unwrap_or(0.),"cores":usages.into_iter().skip(1).collect::<Vec<_>>(),"mem":mem,"net":{"rx":rx,"tx":tx,"rxTotal":rxt,"txTotal":txt,"ifs":ifs},"disk":{"read":rd,"write":wr,"disks":disks},"temp":sensors["temp"],"fan":sensors["fan"],"load":load["load"],"tasks":load["tasks"],"uptime":sensors::uptime(root),"volt":sensors["voltage"]["chips"].as_array().is_some_and(|a|!a.is_empty())||sensors["voltage"]["supplies"].as_array().is_some_and(|a|!a.is_empty()),"pressure":sensors::pressure(&read(&root.join("proc/pressure/memory"),1024))})
    }
}
pub fn info(root: &Path) -> Value {
    let cpu = read(&root.join("proc/cpuinfo"), 1 << 20);
    let model = cpu
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.trim() == "model name")
                .map(|(_, v)| v.trim())
        })
        .unwrap_or("");
    let cores = cpu.lines().filter(|l| l.starts_with("processor\t")).count();
    let zone = std::fs::read_link(root.join("etc/localtime"))
        .unwrap_or_default()
        .to_string_lossy()
        .split("zoneinfo/")
        .last()
        .unwrap_or("")
        .to_owned();
    let tabs = read(&root.join("usr/share/zoneinfo/zone1970.tab"), 1 << 20);
    let home = tabs.lines().filter(|l| !l.starts_with('#')).find_map(|l| {
        let v: Vec<_> = l.split('\t').collect();
        if v.get(2) == Some(&zone.as_str()) {
            Some(json!({"cc":v[0],"zone":zone}))
        } else {
            None
        }
    });
    json!({"cpuModel":model,"cores":cores,"geo":true,"home":home,"tick":super::worker::SAMPLE_MS})
}

use super::*;
#[test]
fn rust_rule_editor_matches_the_shared_firewall_destination_contract() {
    for line in include_str!("../../server/tests/fw_dest_cases.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let (input, want) = line.split_once('\t').unwrap();
        let result = rules::destination(input);
        if want == "!" {
            assert!(result.is_err(), "{input}: {result:?}");
        } else {
            assert_eq!(result.as_deref(), Ok(want), "{input}");
        }
    }
    assert_eq!(rules::port("8000 - 8080"), Ok("8000-8080".into()));
    for port in ["0", "01", "65536", "2-1", "1-2-3", "abc"] {
        assert!(rules::port(port).is_err(), "{port}");
    }
    assert!(rules::validate(&json!({"dst":"any"}), "desktop:test").is_err());
    assert!(rules::validate(
        &json!({"dst":"example.com","port":"443","proto":"tcp","action":"deny"}),
        "desktop:test"
    )
    .is_ok());
}
#[test]
fn native_view_labels_exist_in_every_shipped_language() {
    fn check(node: &Value, catalogs: &Value) {
        if let Some(k) = node.get("key").and_then(Value::as_str) {
            if let Some((ns, id)) = k.split_once('.') {
                assert!(!catalogs[ns][id].is_null(), "missing {k}");
            }
        }
        match node {
            Value::Array(a) => {
                for v in a {
                    check(v, catalogs)
                }
            }
            Value::Object(o) => {
                for v in o.values() {
                    check(v, catalogs)
                }
            }
            _ => {}
        }
    }
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for lang in ["en", "pt", "es"] {
        let mut catalogs = json!({});
        for ns in ["monitor", "firewall", "fwrules", "cooling", "countries"] {
            catalogs[ns] = serde_json::from_str(
                &fs::read_to_string(repo.join(format!("assets/locales/{lang}/{ns}.json"))).unwrap(),
            )
            .unwrap();
        }
        let mut app = App::default();
        app.snapshot = Arc::new(fixture());
        for tab in Tab::ALL {
            app.tab = tab;
            check(&ui::tree(&mut app, &json!({})), &catalogs);
        }
        for section in ["privacy", "firewall", "rules", "blocklists"] {
            app.section = section.into();
            app.app = "lpk:test".into();
            check(&ui::tree(&mut app, &json!({})), &catalogs);
        }
    }
}
#[test]
fn tcp_diag_lengths_and_byte_counters_are_checked() {
    let mut packet = vec![0u8; 16 + 72 + 4 + 136];
    let len = packet.len() as u32;
    packet[..4].copy_from_slice(&len.to_ne_bytes());
    packet[4..6].copy_from_slice(&20u16.to_ne_bytes());
    packet[16 + 68..16 + 72].copy_from_slice(&123u32.to_ne_bytes());
    packet[88..90].copy_from_slice(&140u16.to_ne_bytes());
    packet[90..92].copy_from_slice(&2u16.to_ne_bytes());
    packet[92 + 120..92 + 128].copy_from_slice(&456u64.to_ne_bytes());
    packet[92 + 128..92 + 136].copy_from_slice(&789u64.to_ne_bytes());
    let mut counts = std::collections::HashMap::new();
    connections::diag_messages(&packet, &mut counts);
    assert_eq!(counts.get(&123), Some(&(789, 456)));
    for n in 0..packet.len() {
        connections::diag_messages(&packet[..n], &mut std::collections::HashMap::new());
    }
}
#[test]
#[ignore = "drawing benchmark; run with --release --ignored --nocapture"]
fn native_chart_benchmark() {
    let series = vec![
        (vec![18e9; 240], [196, 181, 253, 255]),
        (vec![6e9; 240], [139, 92, 246, 255]),
    ];
    let mut timings = Vec::new();
    for _ in 0..40 {
        let start = Instant::now();
        std::hint::black_box(charts::area(1840, 170, &series, 24e9, 240, 1));
        timings.push(start.elapsed().as_secs_f64() * 1000.);
    }
    timings.sort_by(f64::total_cmp);
    println!(
        "native Monitor chart: median {:.3} ms, p95 {:.3} ms",
        timings[20], timings[38]
    );
}
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "axia-monitor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn put(&self, name: &str, value: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn available_memory_does_not_treat_file_cache_as_an_application_leak() {
    let m=memory::parse("MemTotal: 24491580 kB\nMemFree: 214200 kB\nMemAvailable: 18482104 kB\nBuffers: 2624 kB\nCached: 18349668 kB\nSReclaimable: 187720 kB\nShmem: 154544 kB\nSwapTotal: 8388604 kB\nSwapFree: 8388604 kB\n");
    assert_eq!(m["avail"], 18482104u64 * 1024);
    assert_eq!(m["used"], (24491580u64 - 18482104) * 1024);
    assert_eq!(m["swapUsed"], 0);
    assert!(m["avail"].as_u64().unwrap() > m["free"].as_u64().unwrap() * 50);
}
#[test]
fn memory_bounds_and_missing_fields_never_underflow() {
    for s in [
        "",
        "MemTotal: 100 kB\nMemAvailable: 200 kB\n",
        "MemTotal: 18446744073709551615 kB\n",
        "MemTotal: 100 kB\nSwapFree: 200 kB\n",
        "MemTotal: 100 kB\nCached: 80 kB\nShmem: 90 kB\n",
    ] {
        let m = memory::parse(s);
        assert!(m["used"].as_u64().unwrap() <= m["total"].as_u64().unwrap());
        assert!(m["cache"].as_u64().unwrap() <= m["avail"].as_u64().unwrap());
    }
}
#[test]
fn proc_stat_accepts_names_with_parentheses_and_refuses_truncation() {
    let s=processes::parse("4242 (a) b (c) S 1 4242 4242 0 -1 4194560 10 0 2 0 7 3 0 0 20 0 5 0 123456 1000000 250 18446744073709551615").unwrap();
    assert_eq!(s.name, "a) b (c");
    assert_eq!(s.start, 123456);
    assert_eq!(s.cpu, 10);
    assert_eq!(s.rss, 250);
    for t in ["garbage", "12 (x) S 1 2", "x)("] {
        assert!(processes::parse(t).is_none());
    }
}
#[test]
fn signal_refuses_pid_one_invalid_signal_and_stale_start() {
    assert_eq!(processes::signal(1, 0, "TERM"), Err("sys.badArgument"));
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let pid = child.id() as i32;
    let s = processes::parse(&read(&PathBuf::from(format!("/proc/{pid}/stat")), 8192)).unwrap();
    assert_eq!(
        processes::signal(pid, s.start.saturating_add(1), "KILL"),
        Err("proc.gone")
    );
    assert!(child.try_wait().unwrap().is_none());
    assert_eq!(
        processes::signal(pid, s.start, "HUP"),
        Err("sys.badArgument")
    );
    assert!(processes::signal(pid, s.start, "TERM").is_ok());
    child.wait().unwrap();
}
#[test]
fn cpu_does_not_double_count_guest_time() {
    assert_eq!(
        metrics::cpu("cpu 100 20 30 400 10 5 5 0 100 20\ncpu0 1 0 0 4\nintr 100\n"),
        vec![(570, 160), (5, 1)]
    );
}
#[test]
fn socket_endpoints_ipv4_ipv6_and_ownership_are_preserved() {
    let v=connections::table("header\n 0: 0100007F:1F90 08080808:01BB 01 00000000:00000000 00:00000000 00000000 1000 0 12345 1\n","tcp");
    assert_eq!(v[0]["local"], "127.0.0.1");
    assert_eq!(v[0]["remote"], "8.8.8.8");
    assert_eq!(v[0]["port"], 443);
    assert_eq!(v[0]["inode"], 12345);
    assert_eq!(connections::scope("192.168.1.1".parse().unwrap()), "lan");
    assert_eq!(connections::scope("::1".parse().unwrap()), "local");
}
#[test]
fn missing_sensors_are_not_zero_readings() {
    let tree = Tree::new();
    tree.put("sys/class/hwmon/hwmon0/name", "coretemp\n");
    tree.put("sys/class/hwmon/hwmon0/temp1_input", "42000\n");
    tree.put("sys/class/hwmon/hwmon0/fan1_input", "1234\n");
    tree.put("sys/class/hwmon/hwmon0/in0_input", "0\n");
    let s = sensors::collect(&tree.0);
    assert_eq!(s["temp"], 42.);
    assert_eq!(s["fan"], 1234.);
    assert!(arr(&s["voltage"]["chips"]).is_empty());
    for text in ["12x", "2\n3", "", "123456789012"] {
        assert!(sensors::integer(text).is_none())
    }
}
#[test]
fn power_supplies_keep_microvolt_scale_and_battery_identity() {
    let tree = Tree::new();
    tree.put("sys/class/power_supply/BAT0/voltage_now", "12123456\n");
    tree.put("sys/class/power_supply/BAT0/type", "Battery\n");
    let s = sensors::collect(&tree.0);
    let rail = ui::primary_voltage(&s["voltage"]).unwrap();
    assert_eq!(rail.0, "BAT0");
    assert!((rail.1 - 12.123456).abs() < 1e-8);
}
#[test]
fn history_roundtrip_crc_corruption_and_hour_distribution() {
    let mut h = history::History::default();
    h.enabled = true;
    h.add(3600, "", 120, 60);
    h.add(3600, "desktop:browser", 120, 60);
    let bytes = h.encode();
    let mut copy = history::History::default();
    copy.decode(&bytes);
    assert_eq!(copy.buckets, h.buckets);
    let q = copy.query(3600, 30, 2, "", 3660);
    assert_eq!(q["rx"], json!([60., 60.]));
    let mut bad = bytes.clone();
    *bad.last_mut().unwrap() ^= 1;
    let mut parsed = history::History::default();
    parsed.decode(&bad);
    assert!(parsed.buckets.len() < h.buckets.len());
    for n in 0..bytes.len() {
        let mut parsed = history::History::default();
        parsed.decode(&bytes[..n]);
    }
    assert_eq!(history::crc(b"123456789"), 0xcbf43926);
}
#[test]
fn history_private_atomic_file_and_clear() {
    let tree = Tree::new();
    let path = tree.0.join("state/net-history.bin");
    let mut h = history::History::new(path.clone());
    h.enabled = true;
    h.add(3600, "", 123, 456);
    h.save(3661).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let mut copy = history::History::new(path);
    copy.load();
    assert_eq!(copy.buckets, h.buckets);
    assert!(copy.clear());
    assert!(copy.buckets.is_empty());
}
#[test]
fn local_country_database_ipv4_ipv6_and_private_addresses() {
    let db =
        geo::Database::parse(include_bytes!("../../assets/monitor/geoip-country.mmdb").to_vec())
            .unwrap();
    for (ip, want) in [
        ("8.8.8.8", Some("US")),
        ("1.1.1.1", Some("AU")),
        ("177.10.0.1", Some("BR")),
        ("2800:810::1", Some("AR")),
        ("2a01:4f8::1", Some("DE")),
        ("10.0.0.1", None),
        ("::1", None),
    ] {
        assert_eq!(db.country(ip.parse().unwrap()).as_deref(), want, "{ip}")
    }
}
#[test]
fn damaged_geo_inputs_are_bounded() {
    let bytes = include_bytes!("../../assets/monitor/geoip-country.mmdb");
    for n in [0, 1, 12, 100, bytes.len() / 2, bytes.len() - 1] {
        if let Some(db) = geo::Database::parse(bytes[..n].to_vec()) {
            let _ = db.country("8.8.8.8".parse().unwrap());
        }
    }
    assert!(geo::outlines(b"bad").is_none());
    assert!(
        !geo::outlines(include_bytes!("../../assets/monitor/land.bin"))
            .unwrap()
            .is_empty()
    );
}
#[test]
fn chart_buffers_are_bounded_deterministic_and_not_json_pixels() {
    let series = vec![(vec![0.5; 240], [139, 92, 246, 255])];
    let a = charts::area(1840, 170, &series, 1., 240, 1);
    let b = charts::area(1840, 170, &series, 1., 240, 2);
    assert_eq!(a.data, b.data);
    assert_eq!(a.data.len(), 1840 * 170 * 4);
    assert!(a.data.chunks_exact(4).any(|p| p[3] > 0));
    let empty = charts::area(0, 0, &[], f64::NAN, 0, 1);
    assert_eq!(empty.data.len(), 4);
}
fn fixture() -> worker::Snapshot {
    let s = json!({"t":1000,"cpu":0.2,"cores":[0.1,0.3],"mem":{"total":24e9,"used":6e9,"avail":18e9,"cache":17e9,"free":1e9,"swapTotal":8e9,"swapUsed":0},"net":{"rx":10,"tx":20,"rxTotal":100,"txTotal":200},"disk":{"read":0,"write":0,"disks":[]},"load":[1,2,3],"tasks":{"running":2,"total":100},"volt":true});
    worker::Snapshot {
        seq: 1,
        samples: VecDeque::from([s.clone(), s]),
        info: json!({"cpuModel":"test","cores":2}),
        sensors: json!({"temps":[],"fans":[],"voltage":{"chips":[],"supplies":[]}}),
        processes: json!({"list":[]}),
        connections: json!({"list":[]}),
        ..Default::default()
    }
}
#[test]
fn all_eight_tabs_and_privacy_views_are_rust_owned() {
    let mut app = App::default();
    app.snapshot = Arc::new(fixture());
    let context = json!({"theme":{"mode":"dark","text":"#fff","separator":"#444"},"width":1180});
    for tab in Tab::ALL {
        app.tab = tab;
        let tree = ui::tree(&mut app, &context);
        assert_eq!(tree["kind"], "view");
        assert!(!arr(&tree["children"]).is_empty());
    }
    for section in ["privacy", "firewall", "rules", "blocklists"] {
        app.section = section.into();
        assert_eq!(ui::tree(&mut app, &context)["kind"], "view");
    }
}
#[test]
fn memory_view_highlights_available_and_preserves_actual_free() {
    let mut app = App::default();
    app.snapshot = Arc::new(fixture());
    app.tab = Tab::Memory;
    let tree = ui::tree(&mut app, &json!({})).to_string();
    assert!(tree.contains("monitor.memAvailable"));
    assert!(tree.contains("16.8 GiB"));
    assert!(tree.contains("monitor.memFree"));
    assert!(!tree.contains("Uint32Array"));
}
#[test]
fn snapshot_publication_keeps_old_readers_alive() {
    let old = Arc::new(fixture());
    let reader = old.clone();
    let mut published = old;
    for i in 0..1000 {
        let mut next = fixture();
        next.seq = i;
        published = Arc::new(next);
    }
    assert_eq!(reader.seq, 1);
    assert_eq!(published.seq, 999);
}
#[test]
fn real_sampler_reports_memory_without_blocking_ui_snapshot() {
    let service = worker::Service::new(PathBuf::from("/"), PathBuf::new());
    let lease = service.lease();
    let start = Instant::now();
    while service.snapshot().samples.is_empty() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let snap = service.snapshot();
    assert!(
        snap.samples.back().unwrap()["mem"]["total"]
            .as_u64()
            .unwrap()
            > 0
    );
    drop(lease);
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn history_query_rejects_overflowing_windows() {
    let mut history = history::History::new(PathBuf::new());
    assert!(history.query(0, u64::MAX, 2, "", 0).is_null());
    assert!(history.query(u64::MAX, 1, 1, "", 0).is_null());
}

#[test]
fn cgroup_app_identity_uses_the_root_owned_session_subtree() {
    assert_eq!(
        connections::app_of_cgroup(
            "0::/system.slice/lordos.service/lordos-session-1000/app-lordos-firefox.scope/child",
            1000
        ),
        "desktop:firefox"
    );
    assert_eq!(
        connections::app_of_cgroup(
            "0::/system.slice/lordos.service/lordos-session-1000/app-lordos-lpk-test.scope",
            1000
        ),
        "lpk:test"
    );
    assert_eq!(
        connections::app_of_cgroup("0::/user.slice/app-lordos-firefox.scope", 1000),
        ""
    );
    assert_eq!(
        connections::app_of_cgroup(
            "0::/system.slice/lordos.service/lordos-session-1001/app-lordos-test.scope",
            1000
        ),
        ""
    );
}
#[test]
fn fan_protocol_is_bounded_and_preserves_daemon_status() {
    use std::{
        io::{Read, Write},
        os::unix::net::UnixListener,
    };
    let tree = Tree::new();
    let path = tree.0.join("fan.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut input = [0; 7];
        stream.read_exact(&mut input).unwrap();
        assert_eq!(&input, b"STATUS\n");
        stream
            .write_all(br#"{"profile":"balanced","force":"off","fans":[]}"#)
            .unwrap();
    });
    let status: Value = serde_json::from_str(&fan::ask(&path, "STATUS").unwrap()).unwrap();
    assert_eq!(status["profile"], "balanced");
    worker.join().unwrap();
}
#[test]
fn rule_editor_keeps_numeric_rule_identifiers() {
    let mut app = App::default();
    app.rules_data = json!([{"id":42,"dst":"example.com","action":"deny"}]);
    app.request(&json!({"cmd":"editRule","key":"42"}));
    assert_eq!(app.form["dst"], "example.com");
    assert_eq!(app.form["editId"], "42");
}

#[test]
fn disk_sensors_match_devices_and_keep_sleeping_disks_unread() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    tree.put("sys/devices/nvme0/marker", "");
    tree.put("sys/devices/subsystem/marker", "");
    tree.put("sys/block/nvme0n1/marker", "");
    symlink(
        tree.0.join("sys/devices/subsystem"),
        tree.0.join("sys/block/nvme0n1/device"),
    )
    .unwrap();
    symlink(
        tree.0.join("sys/devices/nvme0"),
        tree.0.join("sys/devices/subsystem/nvme0"),
    )
    .unwrap();
    tree.put("sys/class/hwmon/hwmon0/name", "nvme\n");
    tree.put("sys/class/hwmon/hwmon0/temp1_input", "42000\n");
    tree.put("sys/class/hwmon/hwmon0/temp1_max", "75000\n");
    tree.put("sys/class/hwmon/hwmon0/temp1_crit", "70000\n");
    symlink(
        tree.0.join("sys/devices/nvme0"),
        tree.0.join("sys/class/hwmon/hwmon0/device"),
    )
    .unwrap();
    let reading = sensors::disk_temperature(&tree.0, "nvme0n1", true);
    assert_eq!(reading["temp"], 42.);
    assert_eq!(reading["tempMax"], 75.);
    assert!(reading["tempCrit"].is_null());
    assert!(sensors::disk_temperature(&tree.0, "nvme0n1", false)["temp"].is_null());
    tree.put("sys/class/hwmon/hwmon0/name", "coretemp\n");
    assert!(sensors::disk_temperature(&tree.0, "nvme0n1", true)["sensor"].is_null());
    assert!(sensors::disk_temperature(&tree.0, "../../devices/nvme0", true)["temp"].is_null());
}
#[test]
fn subscription_notifications_do_not_accelerate_metric_sampling() {
    let tree = Tree::new();
    tree.put("proc/meminfo", "MemTotal: 100 kB\n");
    let service = worker::Service::new(tree.0.clone(), PathBuf::new());
    let lease = service.lease();
    let start = Instant::now();
    while service.snapshot().samples.is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let before = service.snapshot().samples.len();
    assert!(before > 0);
    let notifications = Instant::now();
    for i in 0..20 {
        service.update(
            &lease,
            worker::Want {
                geo: i % 2 == 0,
                ..Default::default()
            },
        );
        std::thread::sleep(Duration::from_millis(3));
    }
    assert!(
        service.snapshot().samples.len()
            <= before + 1 + (notifications.elapsed().as_millis() / 250) as usize
    );
}

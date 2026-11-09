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
        samples: VecDeque::from([std::sync::Arc::new(s.clone()), std::sync::Arc::new(s)]),
        info: json!({"cpuModel":"test","cores":2,"physical":1}),
        system: json!({"host":"axia","os":"Axia Linux","model":"","filesystems":[{"mount":"/","total":1e12,"used":2e11,"avail":8e11},{"mount":"/home","total":5e11,"used":1e11,"avail":4e11}],"network":{"gateway":"192.168.0.1","gatewayMac":"aa:bb:cc:dd:ee:ff","ifs":[{"name":"wlan0","mac":"11:22:33:44:55:66","wireless":true,"up":true,"primary":true,"addrs":["192.168.0.5"]}]},"battery":{"capacity":87.0,"status":"Charging","cycles":18.0,"full":4879.0,"design":4993.0,"unit":"mAh"}}),
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
            <= before
                + 1
                + (notifications.elapsed().as_millis() / worker::SAMPLE_MS as u128) as usize
    );
}
#[test]
fn unchanged_sync_reuses_the_tree_and_any_command_rebuilds_it() {
    let tree = Tree::new();
    tree.put("proc/meminfo", "MemTotal: 100 kB\n");
    let service = worker::Service::new(tree.0.clone(), PathBuf::new());
    let mut app = App {
        lease: Some(service.lease()),
        ..Default::default()
    };
    let sync = json!({"cmd":"sync","context":{"theme":{"mode":"dark"},"width":1180}});
    let start = Instant::now();
    while service.snapshot().samples.is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    // A sample may land between two calls; retry until one pair sees the same data.
    let mut reused = false;
    for _ in 0..20 {
        let first = app.request(&sync);
        assert!(first["tree"].is_object());
        let seq = service.snapshot().seq;
        let second = app.request(&sync);
        if service.snapshot().seq == seq {
            assert_eq!(second["same"], true);
            assert!(
                second["tree"].is_null(),
                "an unchanged tree is not serialized again"
            );
            assert_eq!(second["generation"], first["generation"]);
            reused = true;
            break;
        }
    }
    assert!(reused);
    // A new context or a command invalidates the cached tree.
    let mut wider = sync.clone();
    wider["context"]["width"] = json!(1400);
    assert!(app.request(&wider)["tree"].is_object());
    app.request(&json!({"cmd":"tab","key":"memory"}));
    assert!(app.request(&wider)["tree"].is_object());
}
#[test]
fn filesystems_keep_one_entry_per_device_and_skip_virtual_mounts() {
    let tree = Tree::new();
    tree.put("mnt/My Disk/.keep", "");
    tree.put("home/.keep", "");
    tree.put(
        "proc/mounts",
        "/dev/sdb1 /mnt/My\\040Disk ext4 rw 0 0\n\
         /dev/sda2 / btrfs rw 0 0\n\
         /dev/sda2 /home btrfs rw 0 0\n\
         tmpfs /tmp tmpfs rw 0 0\n\
         /dev/loop0 /snap/core squashfs ro 0 0\n\
         /dev/sda1 /boot/efi vfat rw 0 0\n",
    );
    let list = system::filesystems(&tree.0);
    let mounts: Vec<_> = arr(&list).iter().map(|f| f["mount"].clone()).collect();
    assert_eq!(mounts, [json!("/"), json!("/mnt/My Disk")]);
    for fs in arr(&list) {
        assert!(fs["total"].as_u64().unwrap() > 0);
        assert!(fs["used"].as_u64().unwrap() <= fs["total"].as_u64().unwrap());
    }
}
#[test]
fn network_facts_read_the_default_route_gateway_and_interfaces() {
    let tree = Tree::new();
    // 192.168.2.1 as the kernel prints the network-order word.
    let gw = u32::from_ne_bytes([192, 168, 2, 1]);
    tree.put(
        "proc/net/route",
        &format!("Iface\tDestination\tGateway\tFlags\nwlan0\t00000000\t{gw:08X}\t0003\n"),
    );
    tree.put(
        "proc/net/arp",
        "IP address HW type Flags HW address Mask Device\n192.168.2.1 0x1 0x2 aa:bb:cc:00:11:22 * wlan0\n",
    );
    tree.put("sys/class/net/eth0/address", "00:11:22:33:44:55\n");
    tree.put("sys/class/net/eth0/operstate", "down\n");
    tree.put("sys/class/net/wlan0/address", "66:77:88:99:aa:bb\n");
    tree.put("sys/class/net/wlan0/operstate", "up\n");
    tree.put("sys/class/net/wlan0/wireless/.keep", "");
    tree.put("sys/class/net/lo/address", "00:00:00:00:00:00\n");
    tree.put("sys/class/net/docker0/address", "02:42:00:00:00:00\n");
    tree.put("sys/devices/virtual/net/docker0/.keep", "");
    let net = system::network(&tree.0);
    assert_eq!(net["gateway"], "192.168.2.1");
    assert_eq!(net["gatewayMac"], "aa:bb:cc:00:11:22");
    let names: Vec<_> = arr(&net["ifs"]).iter().map(|i| i["name"].clone()).collect();
    assert_eq!(
        names,
        [json!("wlan0"), json!("eth0")],
        "primary first; no lo or virtual"
    );
    assert_eq!(net["ifs"][0]["wireless"], true);
    assert_eq!(net["ifs"][0]["up"], true);
    assert_eq!(net["ifs"][1]["wireless"], false);
}
#[test]
fn battery_reports_charge_in_mah_and_skips_peripherals() {
    let tree = Tree::new();
    for (k, v) in [("type", "Battery"), ("scope", "Device"), ("capacity", "40")] {
        tree.put(&format!("sys/class/power_supply/hidpp_battery_0/{k}"), v);
    }
    for (k, v) in [
        ("type", "Battery\n"),
        ("status", "Charging\n"),
        ("capacity", "87\n"),
        ("cycle_count", "18\n"),
        ("charge_full", "4879000\n"),
        ("charge_full_design", "4993000\n"),
        ("charge_now", "4250000\n"),
    ] {
        tree.put(&format!("sys/class/power_supply/BAT0/{k}"), v);
    }
    let b = system::battery(&tree.0);
    assert_eq!(b["name"], "BAT0");
    assert_eq!(b["capacity"], 87.);
    assert_eq!(b["status"], "Charging");
    assert_eq!(b["cycles"], 18.);
    assert_eq!(b["unit"], "mAh");
    assert!((b["full"].as_f64().unwrap() - 4879.).abs() < 1e-6);
    assert!((b["design"].as_f64().unwrap() - 4993.).abs() < 1e-6);
    // Energy-reporting batteries use Wh; no battery at all is null.
    let tree = Tree::new();
    for (k, v) in [
        ("type", "Battery"),
        ("energy_full", "50000000"),
        ("energy_now", "25000000"),
    ] {
        tree.put(&format!("sys/class/power_supply/BAT1/{k}"), v);
    }
    let b = system::battery(&tree.0);
    assert_eq!(b["unit"], "Wh");
    assert_eq!(b["capacity"], 50.);
    assert!(b["cycles"].is_null());
    assert!(system::battery(&Tree::new().0).is_null());
}
#[test]
fn os_name_and_model_ignore_firmware_placeholders() {
    let tree = Tree::new();
    tree.put(
        "etc/os-release",
        "NAME=Axia\nPRETTY_NAME=\"Axia Linux 1.0\"\n",
    );
    tree.put("sys/class/dmi/id/product_name", "To Be Filled By O.E.M.\n");
    tree.put("sys/class/dmi/id/board_name", "B450M\n");
    assert_eq!(system::os_name(&tree.0), "Axia Linux 1.0");
    assert_eq!(system::model(&tree.0), "B450M");
    tree.put("sys/class/dmi/id/product_name", "20XW\n");
    tree.put(
        "sys/class/dmi/id/product_version",
        "ThinkPad X1 Carbon Gen 9\n",
    );
    assert_eq!(system::model(&tree.0), "ThinkPad X1 Carbon Gen 9");
}
#[test]
fn kernel_memory_is_part_of_used_and_bounded_by_it() {
    let m = memory::parse(
        "MemTotal: 1000 kB\nMemFree: 100 kB\nMemAvailable: 600 kB\nSUnreclaim: 50 kB\nKernelStack: 10 kB\nPageTables: 20 kB\n",
    );
    assert_eq!(m["used"], 400 * 1024);
    assert_eq!(m["kernel"], 80 * 1024);
    let m = memory::parse("MemTotal: 100 kB\nMemAvailable: 90 kB\nSUnreclaim: 500 kB\n");
    assert_eq!(m["kernel"], 10 * 1024);
}
#[test]
fn icons_and_rings_are_antialiased_and_transparent_outside_the_shapes() {
    for icon in [
        paint::Icon::Dashboard,
        paint::Icon::Processes,
        paint::Icon::Cpu,
        paint::Icon::Memory,
        paint::Icon::Storage,
        paint::Icon::Network,
        paint::Icon::Cooling,
        paint::Icon::Voltage,
        paint::Icon::Settings,
        paint::Icon::Laptop,
        paint::Icon::Router,
    ] {
        let r = paint::icon(icon, 40, [10, 132, 255, 255]);
        let alpha: Vec<u8> = r.data.chunks_exact(4).map(|p| p[3]).collect();
        assert_eq!(alpha[0], 0, "{icon:?}: corner");
        assert!(alpha.iter().any(|&a| a == 255), "{icon:?}: solid strokes");
        assert!(
            alpha.iter().any(|&a| a > 0 && a < 255),
            "{icon:?}: antialiased edges"
        );
        for p in r.data.chunks_exact(4).filter(|p| p[3] > 0) {
            assert_eq!(&p[..3], &[10, 132, 255], "{icon:?}: one colour");
        }
    }
    let rings = paint::rings(
        100,
        &[(40., 8.)],
        &[paint::Ring {
            radius: 40.,
            width: 8.,
            start: 0.,
            end: 0.25,
            color: [255, 0, 0, 255],
        }],
        [0, 0, 0, 40],
        1,
    );
    let at = |x: usize, y: usize| &rings.data[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4];
    assert_eq!(at(50, 50)[3], 0, "centre");
    assert_eq!(at(78, 22), &[255, 0, 0, 255], "inside the first quarter");
    assert_eq!(at(22, 78)[3], 40, "track only in the third quarter");
}
#[test]
fn dashboard_links_open_sorted_processes_and_settings_panels_hide_the_chrome() {
    let mut app = App::default();
    app.snapshot = Arc::new(fixture());
    let context = json!({"theme":{"mode":"light","accent":"#ff0000"},"width":1180});
    let tree = ui::tree(&mut app, &context).to_string();
    for k in [
        "monitor.dash.summary",
        "monitor.dash.battery",
        "monitor.bat.charging",
        "monitor.dash.wifi",
        "monitor.nav.dashboard",
        "axia-icon-128.png",
        "192.168.0.5",
    ] {
        assert!(tree.contains(k), "{k}");
    }
    // Icons are drawn once in the accent colour and survive rebuilds.
    assert!(app.icons.keys().any(|k| k.contains("ff0000")));
    let icons = app.icons.len();
    ui::tree(&mut app, &context);
    assert_eq!(app.icons.len(), icons);
    assert!(app.pixels("summary-rings").is_some());
    app.request(&json!({"cmd":"showApps","key":"rss"}));
    assert_eq!(
        (app.tab, app.sort.as_str(), app.reverse),
        (Tab::Processes, "rss", true)
    );
    app.request(&json!({"cmd":"showApps","key":"name; rm"}));
    assert_eq!(app.sort, "rss", "only known columns");
    // Inside Settings: no sidebar, header or Monitor entry.
    app.args = json!({"section":"privacy"});
    app.section = "privacy".into();
    let embedded = ui::tree(&mut app, &context).to_string();
    assert!(!embedded.contains("monitor.nav.dashboard"));
    assert!(!embedded.contains("axia-icon-128.png"));
    assert!(!embedded.contains("\"monitor.title\""));
}

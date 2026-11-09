//! Bounded /proc collection and PID-reuse-safe process control. No commands or root helpers.
use super::{entries, number, read};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, os::unix::fs::MetadataExt, path::Path};
#[derive(Clone, Debug)]
pub struct Stat {
    pub pid: i32,
    pub name: String,
    pub state: char,
    pub ppid: i32,
    pub flags: u64,
    pub cpu: u64,
    pub start: u64,
    pub rss: u64,
    pub size: u64,
    pub threads: u64,
    pub minor: u64,
    pub major: u64,
    pub priority: i64,
    pub nice: i64,
}
pub fn parse(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close <= open {
        return None;
    }
    let pid = text[..open].trim().parse().ok()?;
    let v: Vec<_> = text[close + 1..].split_whitespace().collect();
    let n = |i: usize| v.get(i)?.parse::<u64>().ok();
    Some(Stat {
        pid,
        name: text[open + 1..close].into(),
        state: v.first()?.chars().next()?,
        ppid: v.get(1)?.parse().ok()?,
        flags: n(6)?,
        minor: n(7)?,
        major: n(9)?,
        cpu: n(11)?.saturating_add(n(12)?),
        priority: v.get(15)?.parse().ok()?,
        nice: v.get(16)?.parse().ok()?,
        threads: n(17)?,
        start: n(19)?,
        size: n(20)?,
        rss: n(21)?,
    })
}
pub fn protected(s: &Stat, exe: &str, args: &str, session: i32) -> Option<&'static str> {
    if s.pid <= 1 {
        return Some("init");
    }
    if s.flags & 0x00200000 != 0 {
        return Some("kernel");
    }
    if s.pid == session {
        return Some("session");
    }
    let name = Path::new(exe.trim_end_matches(" (deleted)"))
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&s.name);
    if (name == "axia-server" && !args.split('\0').any(|a| a == "--app"))
        || [
            "systemd",
            "pipewire",
            "pipewire-pulse",
            "wireplumber",
            "axia-vault",
            "axia-maild",
            "axia-doc",
            "axia-media",
            "axia-netperm",
            "axia-fand",
            "lord-sessiond",
            "lord-netd",
            "lord-admin",
        ]
        .contains(&name)
    {
        Some("system")
    } else {
        None
    }
}
#[derive(Default)]
pub struct Collector {
    previous: HashMap<i32, (u64, u64)>,
    at: f64,
}
impl Collector {
    pub fn scan(&mut self, root: &Path, now: f64) -> Value {
        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
        let uid = unsafe { libc::getuid() };
        let session = std::process::id() as i32;
        let mut next = HashMap::new();
        let mut list = Vec::new();
        for p in entries(&root.join("proc")).into_iter().take(20000) {
            let Some(pid) = p
                .file_name()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<i32>().ok())
            else {
                continue;
            };
            let Some(s) = parse(&read(&p.join("stat"), 8192)) else {
                continue;
            };
            let owner = fs::metadata(&p).map(|m| m.uid()).unwrap_or(u32::MAX);
            let exe = fs::read_link(p.join("exe"))
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let cmd = read(&p.join("cmdline"), 4096);
            let cpu = self
                .previous
                .get(&pid)
                .filter(|v| v.0 == s.start)
                .map(|v| s.cpu.saturating_sub(v.1) as f64 / hz / (now - self.at).max(0.001))
                .unwrap_or(0.);
            next.insert(pid, (s.start, s.cpu));
            let prot = protected(&s, &exe, &cmd, session);
            list.push(json!({"pid":pid,"name":s.name,"state":s.state.to_string(),"ppid":s.ppid,"threads":s.threads,"start":s.start as f64/hz,"st":s.start,"cpu":cpu,"rss":s.rss.saturating_mul(page),"uid":owner,"user":owner.to_string(),"own":owner==uid,"exe":exe,"cmd":cmd.replace('\0'," "),"self":pid==session,"prot":prot,"app":if pid==session{"lordos"}else{""}}));
        }
        self.previous = next;
        self.at = now;
        json!({"t":now*1000.,"total":list.len(),"list":list})
    }
}
pub fn detail(root: &Path, pid: i32, start: u64) -> Value {
    let p = root.join(format!("proc/{pid}"));
    let Some(s) = parse(&read(&p.join("stat"), 8192)).filter(|s| s.start == start) else {
        return json!({"pid":pid,"gone":true});
    };
    let status = read(&p.join("status"), 32768);
    let io = read(&p.join("io"), 8192);
    let own = fs::metadata(&p)
        .map(|m| m.uid() == unsafe { libc::getuid() })
        .unwrap_or(false);
    let mem = read(&p.join("smaps_rollup"), 32768);
    let mut files = Vec::new();
    if own {
        for f in entries(&p.join("fd")).into_iter().take(1024) {
            if let Ok(target) = fs::read_link(&f) {
                files.push(json!({"fd":f.file_name().unwrap().to_string_lossy(),"path":target.to_string_lossy()}));
            }
        }
    }
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
    json!({"pid":pid,"st":start,"name":s.name,"state":s.state.to_string(),"gone":false,"own":own,"ppid":s.ppid,"threads":s.threads,"priority":s.priority,"nice":s.nice,"minor":s.minor,"major":s.major,"cpuSeconds":s.cpu as f64/hz,"rss":number(&status,"VmRSS:")*1024.,"vsize":s.size,"pss":number(&mem,"Pss:")*1024.,"swap":number(&mem,"Swap:")*1024.,"anon":number(&status,"RssAnon:")*1024.,"shared":number(&status,"RssShmem:")*1024.,"private":(number(&mem,"Private_Clean:")+number(&mem,"Private_Dirty:"))*1024.,"readBytes":number(&io,"read_bytes:"),"writeBytes":number(&io,"write_bytes:"),"exe":fs::read_link(p.join("exe")).ok(),"cwd":fs::read_link(p.join("cwd")).ok(),"cmd":read(&p.join("cmdline"),8192).replace('\0'," "),"files":files})
}
pub fn signal(pid: i32, start: u64, kind: &str) -> Result<(), &'static str> {
    if !(2..=4194304).contains(&pid) || !matches!(kind, "TERM" | "KILL") {
        return Err("sys.badArgument");
    }
    let p = Path::new("/proc").join(pid.to_string());
    if fs::metadata(&p).map_err(|_| "proc.gone")?.uid() != unsafe { libc::getuid() } {
        return Err("proc.notOwn");
    }
    // Open first, then re-check start time, then signal the descriptor. Never use kill(pid).
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
    if fd < 0 {
        return Err("proc.gone");
    }
    let result = (|| {
        let s = parse(&read(&p.join("stat"), 8192)).ok_or("proc.gone")?;
        if s.start != start {
            return Err("proc.gone");
        }
        let exe = fs::read_link(p.join("exe")).unwrap_or_default();
        let args = read(&p.join("cmdline"), 4096);
        if let Some(reason) =
            protected(&s, &exe.to_string_lossy(), &args, std::process::id() as i32)
        {
            return Err(match reason {
                "init" => "proc.protectedInit",
                "kernel" => "proc.protectedKernel",
                "session" => "proc.protectedSession",
                _ => "proc.protectedSystem",
            });
        }
        let rc = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd,
                if kind == "TERM" {
                    libc::SIGTERM
                } else {
                    libc::SIGKILL
                },
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err("proc.denied")
        }
    })();
    unsafe { libc::close(fd) };
    result
}

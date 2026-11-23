//! The sound client against a fake PulseAudio server on a private socket: the handshake, the
//! state it builds and reports as JSON, the commands it sends, the level meter and the system sounds.
//! The client keeps one process-wide state, so everything runs in one test.

use axia_sound::pulse::*;
use axia_sound::snd::*;
use std::collections::VecDeque;
use std::ffi::{c_char, CStr};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Log = Arc<Mutex<Vec<(u32, Vec<u8>)>>>;

fn reply(tag: u32, body: impl FnOnce(&mut Writer)) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(PA_CMD_REPLY);
    w.u32(tag);
    body(&mut w);
    w.finish()
}

fn device(w: &mut Writer, index: u32, name: &str, desc: &str, volumes: &[u32], mute: bool, monitor: u32, ports: &[(&str, &str, u32, u32)], active: &str) {
    w.u32(index);
    w.str(Some(name));
    w.str(Some(desc));
    w.ss(PA_SAMPLE_S16LE, 2, 44100);
    w.cmap(&[1, 2]);
    w.u32(7); // module
    w.cvol(volumes);
    w.bool(mute);
    w.u32(monitor);
    w.str(Some("monitor"));
    w.u64(0);
    w.str(Some("driver"));
    w.u32(0); // flags
    w.proplist(&[("device.icon_name", "audio-card-pci"), ("device.form_factor", "headphones"), ("device.bus", "usb"), ("other", "x")]);
    w.u64(0);
    w.vol(PA_VOLUME_NORM);
    w.u32(1); // state
    w.u32(65537);
    w.u32(3); // card
    w.u32(ports.len() as u32);
    for (n, d, prio, avail) in ports {
        w.str(Some(n));
        w.str(Some(d));
        w.u32(*prio);
        w.u32(*avail);
    }
    w.str(Some(active));
    w.u8(1);
    w.format_pcm();
}

#[allow(clippy::too_many_arguments)]
fn stream(w: &mut Writer, output: bool, index: u32, name: &str, dev: u32, volumes: &[u32], app: &str, appid: &str, pid: &str, media_icon: &str) {
    w.u32(index);
    w.str(Some(name));
    w.u32(1);
    w.u32(2); // client
    w.u32(dev);
    w.ss(PA_SAMPLE_S16LE, 2, 44100);
    w.cmap(&[1, 2]);
    if !output {
        w.cvol(volumes);
    }
    w.u64(0);
    w.u64(0);
    w.str(Some("copy"));
    w.str(Some("driver"));
    if !output {
        w.bool(false);
    }
    w.proplist(&[("application.name", app), ("application.id", appid), ("application.process.id", pid), ("media.icon_name", media_icon), ("media.role", "music")]);
    w.bool(false); // corked
    if output {
        w.cvol(volumes);
        w.bool(true);
    }
    w.bool(true); // has volume
    w.bool(true); // writable
    w.format_pcm();
}

fn serve(mut s: UnixStream, log: Log, pushed: Arc<Mutex<VecDeque<Vec<u8>>>>) {
    s.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
    let mut pending = Vec::new();
    loop {
        // anything the test wants to send (events, meter data)
        while let Some(p) = pushed.lock().unwrap().pop_front() {
            if s.write_all(&p).is_err() {
                return;
            }
        }
        let mut buf = [0u8; 65536];
        match s.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => pending.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
        while pending.len() >= PA_DESCRIPTOR_SIZE {
            let (len, channel, _, _) = descriptor(&pending);
            if pending.len() < PA_DESCRIPTOR_SIZE + len as usize {
                break;
            }
            let body = pending[PA_DESCRIPTOR_SIZE..PA_DESCRIPTOR_SIZE + len as usize].to_vec();
            pending.drain(..PA_DESCRIPTOR_SIZE + len as usize);
            if channel != 0xffff_ffff {
                log.lock().unwrap().push((u32::MAX, body));
                continue;
            }
            let mut r = Reader::new(&body);
            let (cmd, tag) = (r.u32(), r.u32());
            log.lock().unwrap().push((cmd, body[10..].to_vec()));
            let out = match cmd {
                PA_CMD_AUTH => reply(tag, |w| w.u32(32)),
                PA_CMD_GET_SERVER_INFO => reply(tag, |w| {
                    w.str(Some("PulseAudio (on PipeWire 1.2)"));
                    w.str(Some("15.0.0"));
                    w.str(Some("user"));
                    w.str(Some("host"));
                    w.ss(PA_SAMPLE_S16LE, 2, 44100);
                    w.str(Some("sink.main"));
                    w.str(Some("source.mic"));
                }),
                PA_CMD_GET_SINK_INFO_LIST => reply(tag, |w| {
                    device(w, 36, "sink.main", "Built-in \"Audio\"", &[PA_VOLUME_NORM, PA_VOLUME_NORM / 2], false, 40, &[("analog-output-speaker", "Speakers", 100, 2), ("analog-output-headphones", "Headphones", 50, 1), ("hdmi", "HDMI", 10, 0)], "analog-output-speaker");
                    device(w, 37, "sink.hdmi", "", &[PA_VOLUME_NORM], true, PA_INVALID_INDEX, &[], "");
                }),
                PA_CMD_GET_SOURCE_INFO_LIST => reply(tag, |w| {
                    device(w, 40, "source.mic", "Microphone", &[PA_VOLUME_NORM * 3 / 4], false, 36, &[], "");
                    device(w, 41, "source.main.monitor", "Monitor", &[PA_VOLUME_NORM], false, 36, &[], "");
                }),
                PA_CMD_GET_SINK_INPUT_INFO_LIST => reply(tag, |w| {
                    stream(w, false, 51, "Playback", 36, &[PA_VOLUME_NORM], "Player", "org.example.Player", "4242", "multimedia-player");
                    stream(w, false, 52, "System sound", 36, &[PA_VOLUME_NORM], "LordOS", "org.lordos.Server", "1", "");
                }),
                PA_CMD_GET_SOURCE_OUTPUT_INFO_LIST => reply(tag, |w| stream(w, true, 61, "Recording", 40, &[PA_VOLUME_NORM / 2], "Recorder", "", "77", "")),
                PA_CMD_CREATE_RECORD_STREAM => reply(tag, |w| w.u32(9)),
                PA_CMD_CREATE_UPLOAD_STREAM => reply(tag, |w| {
                    w.u32(3);
                    w.u32(0);
                }),
                _ => reply(tag, |_| {}),
            };
            if s.write_all(&out).is_err() {
                return;
            }
        }
    }
}

fn pump(until: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end && !until() {
        let mut p = libc_pollfd(snd_fd());
        // SAFETY: polling one descriptor we own, for a short time.
        unsafe { poll(&mut p, 1, 20) };
        snd_dispatch();
    }
    assert!(until(), "timed out");
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
fn libc_pollfd(fd: i32) -> PollFd {
    PollFd { fd, events: 1, revents: 0 }
}
extern "C" {
    fn poll(fds: *mut PollFd, n: u64, timeout: i32) -> i32;
}

static EVENTS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
unsafe extern "C" fn on_event(kind: *const c_char, json: *const c_char) {
    EVENTS.lock().unwrap().push((CStr::from_ptr(kind).to_string_lossy().into_owned(), CStr::from_ptr(json).to_string_lossy().into_owned()));
}

fn last_state() -> serde_json::Value {
    let events = EVENTS.lock().unwrap();
    let json = events.iter().rev().find(|(k, _)| k == "state").expect("a state event");
    serde_json::from_str(&json.1).unwrap()
}

fn commands(log: &Log, cmd: u32) -> Vec<Vec<u8>> {
    log.lock().unwrap().iter().filter(|(c, _)| *c == cmd).map(|(_, b)| b.clone()).collect()
}

#[test]
fn the_client_builds_the_state_sends_commands_and_reads_the_meter() {
    let dir = std::env::temp_dir().join(format!("axia-sound-test-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("pulse")).unwrap();
    let listener = UnixListener::bind(dir.join("pulse/native")).unwrap();
    std::env::set_var("XDG_RUNTIME_DIR", &dir);
    std::env::remove_var("PULSE_SERVER");
    let (log, pushed): (Log, Arc<Mutex<VecDeque<Vec<u8>>>>) = (Arc::default(), Arc::default());
    let (l2, p2) = (log.clone(), pushed.clone());
    std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        serve(s, l2, p2);
    });

    assert_eq!(snd_start(), 0);
    let report = || unsafe { snd_events(Some(on_event)) };
    pump(|| {
        report();
        EVENTS.lock().unwrap().iter().any(|(k, j)| k == "state" && j.contains("\"connected\":true") && j.contains("\"outputs\":[{"))
    });
    let state = last_state();
    assert_eq!(state["server"], "PulseAudio (on PipeWire 1.2)");
    assert_eq!((state["defaultSink"].as_str(), state["defaultSource"].as_str(), state["meter"].as_i64()), (Some("sink.main"), Some("source.mic"), Some(-1)));
    let sinks = state["sinks"].as_array().unwrap();
    assert_eq!(sinks.len(), 2);
    assert_eq!(sinks[0]["description"], "Built-in \"Audio\"");
    assert_eq!(sinks[0]["volume"], serde_json::json!([1.0, 0.5]));
    assert_eq!(sinks[0]["level"], 1.0);
    assert_eq!((sinks[0]["icon"].as_str(), sinks[0]["formFactor"].as_str(), sinks[0]["bus"].as_str()), (Some("audio-card-pci"), Some("headphones"), Some("usb")));
    assert_eq!((sinks[0]["default"].as_bool(), sinks[1]["default"].as_bool()), (Some(true), Some(false)));
    assert_eq!(sinks[0]["monitor"], 40);
    assert_eq!(sinks[1]["monitor"], -1);
    assert_eq!(sinks[1]["description"], "sink.hdmi", "an empty description shows the name");
    assert_eq!(sinks[1]["mute"], true);
    let available: Vec<_> = sinks[0]["ports"].as_array().unwrap().iter().map(|p| p["available"].clone()).collect();
    assert_eq!(available, [serde_json::json!(true), serde_json::json!(false), serde_json::Value::Null]);
    assert_eq!(sinks[0]["activePort"], "analog-output-speaker");
    let sources = state["sources"].as_array().unwrap();
    assert_eq!((sources[0]["name"].as_str(), sources[0]["monitorOf"].as_i64(), sources[0]["default"].as_bool()), (Some("source.mic"), Some(36), Some(true)));
    // our own streams are not shown; the app name and the icon fall back as before
    let inputs = state["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!((inputs[0]["app"].as_str(), inputs[0]["pid"].as_u64(), inputs[0]["icon"].as_str(), inputs[0]["sink"].as_i64()), (Some("Player"), Some(4242), Some("multimedia-player"), Some(36)));
    let outputs = state["outputs"].as_array().unwrap();
    assert_eq!((outputs[0]["app"].as_str(), outputs[0]["source"].as_i64(), outputs[0]["volume"][0].as_f64()), (Some("Recorder"), Some(40), Some(0.5)));

    // the handshake the server saw
    let auth = commands(&log, PA_CMD_AUTH);
    assert_eq!(auth.len(), 1);
    assert_eq!(commands(&log, PA_CMD_SET_CLIENT_NAME).len(), 1);
    assert_eq!(commands(&log, PA_CMD_SUBSCRIBE).len(), 1);

    // commands: volume (the last value is repeated for the missing channels), level, mute, default, port, move
    assert_eq!(unsafe { snd_set_volume(SND_SINK, 36, [0.5].as_ptr(), 1) }, 0);
    assert_eq!(snd_set_level(SND_SINK, 36, 0.8), 0);
    assert_eq!(snd_set_mute(SND_INPUT, 51, 1), 0);
    assert_eq!(unsafe { snd_set_default(SND_SINK, c"sink.hdmi".as_ptr()) }, 0);
    assert_eq!(unsafe { snd_set_default(SND_SINK, c"nope".as_ptr()) }, -1);
    assert_eq!(unsafe { snd_set_port(SND_SINK, 36, c"hdmi".as_ptr()) }, 0);
    assert_eq!(unsafe { snd_set_port(SND_SINK, 36, c"nope".as_ptr()) }, -1);
    assert_eq!(snd_move(SND_INPUT, 51, 37), 0);
    assert_eq!(snd_move(SND_INPUT, 51, 99), -1);
    assert_eq!(snd_set_mute(9, 1, 1), -1);
    pump(|| commands(&log, PA_CMD_MOVE_SINK_INPUT).len() == 1);
    let vol_cmds = commands(&log, PA_CMD_SET_SINK_VOLUME);
    assert_eq!(vol_cmds.len(), 2);
    let decode = |b: &[u8]| {
        let mut r = Reader::new(b);
        let index = r.u32();
        assert_eq!(r.str(), None);
        let mut v = [0u32; PA_CHANNELS_MAX];
        let n = r.cvol(&mut v);
        assert!(!r.err && r.eof());
        (index, v[..n as usize].to_vec())
    };
    assert_eq!(decode(&vol_cmds[0]), (36, vec![PA_VOLUME_NORM / 2, PA_VOLUME_NORM / 2]));
    assert_eq!(decode(&vol_cmds[1]), (36, vec![52429, 26214]));
    let mute = commands(&log, PA_CMD_SET_SINK_INPUT_MUTE);
    let mut r = Reader::new(&mute[0]);
    assert_eq!((r.u32(), r.bool()), (51, true));
    let def = commands(&log, PA_CMD_SET_DEFAULT_SINK);
    assert_eq!(Reader::new(&def[0]).str(), Some(&b"sink.hdmi"[..]));
    let port = commands(&log, PA_CMD_SET_SINK_PORT);
    let mut r = Reader::new(&port[0]);
    assert_eq!((r.u32(), r.str(), r.str()), (36, None, Some(&b"hdmi"[..])));
    let moved = commands(&log, PA_CMD_MOVE_SINK_INPUT);
    let mut r = Reader::new(&moved[0]);
    assert_eq!((r.u32(), r.u32(), r.str()), (51, 37, None));

    // the meter: a record stream on the source; its data packets give the peak
    assert_eq!(snd_meter(99), -1);
    assert_eq!(snd_meter(40), 0);
    pump(|| !commands(&log, PA_CMD_CREATE_RECORD_STREAM).is_empty());
    std::thread::sleep(Duration::from_millis(100));
    snd_dispatch();
    let samples: Vec<u8> = [0.1f32, -0.7, 0.3].iter().flat_map(|f| f.to_ne_bytes()).collect();
    pushed.lock().unwrap().push_back(data_packet(9, &samples));
    pump(|| {
        report();
        EVENTS.lock().unwrap().iter().any(|(k, _)| k == "peak")
    });
    let peak = EVENTS.lock().unwrap().iter().find(|(k, _)| k == "peak").unwrap().1.clone();
    assert_eq!(peak, "{\"source\":40,\"level\":0.7000}");
    // the state names the meter's source as soon as it is asked for (no "state" event goes with it)
    let json = unsafe {
        let p = snd_state_json();
        let text = CStr::from_ptr(p).to_string_lossy().into_owned();
        libc::free(p as *mut libc::c_void);
        text
    };
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json).unwrap()["meter"], 40);
    assert_eq!(snd_meter(PA_INVALID_INDEX), 0);
    pump(|| !commands(&log, PA_CMD_DELETE_RECORD_STREAM).is_empty());

    // system sounds: upload once, then play (a second time without uploading again)
    assert_eq!(unsafe { snd_play(c"nope".as_ptr()) }, -1);
    assert_eq!(unsafe { snd_play(c"pop".as_ptr()) }, 0);
    pump(|| !commands(&log, PA_CMD_PLAY_SAMPLE).is_empty());
    assert_eq!(unsafe { snd_play(c"pop".as_ptr()) }, 0);
    pump(|| commands(&log, PA_CMD_PLAY_SAMPLE).len() == 2);
    assert_eq!(commands(&log, PA_CMD_CREATE_UPLOAD_STREAM).len(), 1);
    assert_eq!(commands(&log, PA_CMD_FINISH_UPLOAD_STREAM).len(), 1);
    let uploaded: usize = log.lock().unwrap().iter().filter(|(c, _)| *c == u32::MAX).map(|(_, b)| b.len()).sum();
    assert_eq!(uploaded, synth(0).len() * 2);

    snd_stop();
    assert_eq!(snd_fd(), -1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_synthesized_sounds_have_their_lengths_and_stay_in_range() {
    for (which, samples) in [(0usize, 3087usize), (1, 14112), (2, 13230)] {
        let s = synth(which);
        assert_eq!(s.len(), samples, "sound {which}");
        assert!(s.iter().any(|&v| v != 0) && s.iter().all(|&v| i32::from(v).abs() < 20000));
    }
}

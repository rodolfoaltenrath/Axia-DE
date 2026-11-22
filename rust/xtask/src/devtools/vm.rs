//! Driving a Proxmox VM from its host: absolute mouse input and recording over QMP, and typing
//! with the QEMU monitor's `sendkey`. These run on the Proxmox host, not on the development machine.
//!
//!   cargo xtask qmp-input VMID W H CMD...
//!       move X Y | down | up | rdown | rup | wheelup | wheeldown | sleep S
//!   cargo xtask vm-type VMID 'ls -la\n'
//!       special keys in braces: {ret} {tab} {esc} {up} {down} {left} {right} {f12} {ctrl-c} {meta_l-t} …
//!       (any `sendkey` name); the guest must use the US layout
//!   cargo xtask vm-record VMID FOLDER [FPS]
//!       a PNG sequence of the screen (5 fps by default) until FOLDER/STOP is created, or 30 minutes

use crate::{at, Result};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

struct Qmp {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Qmp {
    fn open(vmid: &str, timeout: Option<Duration>) -> Result<Self> {
        let path = format!("/var/run/qemu-server/{vmid}.qmp");
        let stream = at(Path::new(&path), UnixStream::connect(&path))?;
        stream.set_read_timeout(timeout).map_err(|e| e.to_string())?;
        stream.set_write_timeout(timeout).map_err(|e| e.to_string())?;
        let mut qmp = Self { reader: BufReader::new(stream.try_clone().map_err(|e| e.to_string())?), writer: stream };
        let mut greeting = String::new();
        qmp.reader.read_line(&mut greeting).map_err(|e| e.to_string())?;
        qmp.call(json!({"execute": "qmp_capabilities"}))?;
        Ok(qmp)
    }

    /// Sends one command and waits for its `return` or `error` (events in between are skipped).
    fn call(&mut self, command: Value) -> Result<Value> {
        writeln!(self.writer, "{command}").map_err(|e| e.to_string())?;
        self.writer.flush().map_err(|e| e.to_string())?;
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("QMP closed the connection".into());
            }
            let reply: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            if reply.get("return").is_some() || reply.get("error").is_some() {
                return Ok(reply);
            }
        }
    }

    fn event(&mut self, events: Value) -> Result<()> {
        self.call(json!({"execute": "input-send-event", "arguments": {"events": events}}))?;
        std::thread::sleep(Duration::from_millis(50));
        Ok(())
    }
}

/// One step of an input script.
#[derive(Debug, PartialEq)]
pub enum Step {
    Move(i64, i64),
    Button(&'static str, bool),
    Wheel(&'static str),
    Sleep(f64),
}

pub fn parse_steps(cmds: &[String]) -> std::result::Result<Vec<Step>, String> {
    let (mut steps, mut i) = (Vec::new(), 0);
    let number = |i: usize| -> std::result::Result<&String, String> { cmds.get(i).ok_or_else(|| format!("{} needs a value", cmds[i - 1])) };
    while i < cmds.len() {
        match cmds[i].as_str() {
            "move" => {
                let (x, y) = (number(i + 1)?.parse().map_err(|_| "bad X")?, number(i + 2)?.parse().map_err(|_| "bad Y")?);
                steps.push(Step::Move(x, y));
                i += 3;
            }
            c @ ("down" | "up" | "rdown" | "rup") => {
                steps.push(Step::Button(if c.starts_with('r') { "right" } else { "left" }, c.ends_with("down")));
                i += 1;
            }
            c @ ("wheelup" | "wheeldown") => {
                steps.push(Step::Wheel(if c == "wheelup" { "wheel-up" } else { "wheel-down" }));
                i += 1;
            }
            "sleep" => {
                steps.push(Step::Sleep(number(i + 1)?.parse().map_err(|_| "bad seconds")?));
                i += 2;
            }
            _ => i += 1, // the old tool skipped what it did not know
        }
    }
    Ok(steps)
}

/// A pointer position as a QMP absolute axis value (0..32767 over the screen).
pub fn axis(value: i64, size: i64) -> i64 {
    value * 32767 / (size - 1)
}

pub fn qmp_input(args: &[String]) -> Result<ExitCode> {
    let [vmid, w, h, cmds @ ..] = args else { return Err("usage: qmp-input VMID W H CMD...".into()) };
    let (w, h): (i64, i64) = (w.parse().map_err(|_| "bad W")?, h.parse().map_err(|_| "bad H")?);
    if w < 2 || h < 2 {
        return Err("the screen must be at least 2x2".into());
    }
    let steps = parse_steps(cmds)?;
    let mut qmp = Qmp::open(vmid, None)?;
    for step in steps {
        match step {
            Step::Move(x, y) => qmp.event(json!([{"type": "abs", "data": {"axis": "x", "value": axis(x, w)}}, {"type": "abs", "data": {"axis": "y", "value": axis(y, h)}}]))?,
            Step::Button(button, down) => qmp.event(json!([{"type": "btn", "data": {"button": button, "down": down}}]))?,
            Step::Wheel(button) => {
                qmp.event(json!([{"type": "btn", "data": {"button": button, "down": true}}]))?;
                qmp.event(json!([{"type": "btn", "data": {"button": button, "down": false}}]))?;
            }
            Step::Sleep(seconds) => std::thread::sleep(Duration::from_secs_f64(seconds.max(0.0))),
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ---- typing ----

const SHIFTED: [(char, &str); 21] = [
    ('!', "1"), ('@', "2"), ('#', "3"), ('$', "4"), ('%', "5"), ('^', "6"), ('&', "7"), ('*', "8"), ('(', "9"), (')', "0"), ('_', "minus"), ('+', "equal"), ('{', "bracket_left"),
    ('}', "bracket_right"), ('|', "backslash"), (':', "semicolon"), ('"', "apostrophe"), ('<', "comma"), ('>', "dot"), ('?', "slash"), ('~', "grave_accent"),
];
const PLAIN: [(char, &str); 14] = [
    (' ', "spc"), ('-', "minus"), ('=', "equal"), ('[', "bracket_left"), (']', "bracket_right"), ('\\', "backslash"), (';', "semicolon"), ('\'', "apostrophe"), (',', "comma"), ('.', "dot"),
    ('/', "slash"), ('`', "grave_accent"), ('\n', "ret"), ('\t', "tab"),
];

/// The `sendkey` names that type `text` (US layout); `{name}` sends a key by its QEMU name.
pub fn keys(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < chars.len() {
        let c = chars[i];
        if c == '{' {
            if let Some(len) = chars[i..].iter().position(|&x| x == '}') {
                out.push(chars[i + 1..i + len].iter().collect());
                i += len + 1;
                continue;
            }
        }
        if c.is_ascii_alphabetic() {
            out.push(if c.is_ascii_uppercase() { format!("shift-{}", c.to_ascii_lowercase()) } else { c.to_string() });
        } else if c.is_ascii_digit() {
            out.push(c.to_string());
        } else if let Some((_, k)) = SHIFTED.iter().find(|(x, _)| *x == c) {
            out.push(format!("shift-{k}"));
        } else if let Some((_, k)) = PLAIN.iter().find(|(x, _)| *x == c) {
            out.push((*k).to_string());
        }
        i += 1;
    }
    out
}

/// Python's `unicode_escape` decoding of the common escapes: `\n`, `\t`, `\\`, `\xNN`.
pub fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some('x') => {
                let hex: String = (0..2).filter_map(|_| chars.next()).collect();
                out.push(u8::from_str_radix(&hex, 16).map_or('?', char::from));
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

pub fn vm_type(args: &[String]) -> Result<ExitCode> {
    let [vmid, text] = args else { return Err("usage: vm-type VMID TEXT".into()) };
    let list = keys(&unescape(text));
    // In batches: QEMU's keyboard queue drops keys with long bursts.
    for batch in list.chunks(40) {
        let commands: String = batch.iter().map(|k| format!("sendkey {k}\n")).collect();
        let mut child = Command::new("qm").args(["monitor", vmid]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("qm: {e}"))?;
        child.stdin.take().ok_or("no stdin")?.write_all(commands.as_bytes()).map_err(|e| e.to_string())?;
        if !child.wait().map_err(|e| e.to_string())?.success() {
            return Err("qm monitor failed".into());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Ok(ExitCode::SUCCESS)
}

// ---- recording ----

/// One frame: connect, `screendump`, disconnect right away, so Proxmox's own use of the socket is not blocked.
fn screendump(vmid: &str, file: &Path) -> Result<bool> {
    let mut qmp = Qmp::open(vmid, Some(Duration::from_secs(3)))?;
    let reply = qmp.call(json!({"execute": "screendump", "arguments": {"filename": file.to_string_lossy(), "format": "png"}}))?;
    Ok(reply.get("return").is_some())
}

pub fn vm_record(args: &[String]) -> Result<ExitCode> {
    let [vmid, folder, rest @ ..] = args else { return Err("usage: vm-record VMID FOLDER [FPS]".into()) };
    let fps: f64 = rest.first().map_or(Ok(5.0), |f| f.parse().map_err(|_| "bad FPS".to_string()))?;
    if fps <= 0.0 {
        return Err("FPS must be positive".into());
    }
    let out = Path::new(folder);
    at(out, fs::create_dir_all(out))?;
    let mut log = at(&out.join("frames.txt"), fs::File::create(out.join("frames.txt")))?;
    let (t0, mut n) = (Instant::now(), 0u32);
    while !out.join("STOP").exists() && t0.elapsed() < Duration::from_secs(1800) {
        let start = Instant::now();
        let name = format!("f{n:06}.png");
        match screendump(vmid, &out.join(&name)) {
            Ok(true) => {
                writeln!(log, "{:.3} {name}", (start - t0).as_secs_f64()).map_err(|e| e.to_string())?;
                log.flush().map_err(|e| e.to_string())?;
                n += 1;
            }
            Ok(false) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(200)), // the VM is rebooting
        }
        let spent = start.elapsed().as_secs_f64();
        std::thread::sleep(Duration::from_secs_f64((1.0 / fps - spent).max(0.0)));
    }
    println!("{n} frames in {:.0} s", t0.elapsed().as_secs_f64());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn input_scripts_become_steps_and_positions_scale_to_the_axis() {
        let steps = parse_steps(&strings(&["move", "640", "300", "down", "up", "rdown", "rup", "wheelup", "sleep", "0.5", "unknown", "wheeldown"])).unwrap();
        assert_eq!(
            steps,
            [Step::Move(640, 300), Step::Button("left", true), Step::Button("left", false), Step::Button("right", true), Step::Button("right", false), Step::Wheel("wheel-up"), Step::Sleep(0.5), Step::Wheel("wheel-down")]
        );
        assert!(parse_steps(&strings(&["move", "1"])).is_err() && parse_steps(&strings(&["move", "a", "b"])).is_err());
        assert_eq!((axis(0, 1280), axis(1279, 1280), axis(640, 1280)), (0, 32767, 16396));
    }

    #[test]
    fn text_becomes_us_layout_keys_and_braces_send_named_keys() {
        assert_eq!(keys("ls -la\n"), ["l", "s", "spc", "minus", "l", "a", "ret"]);
        assert_eq!(keys("Hi!"), ["shift-h", "i", "shift-1"]);
        assert_eq!(keys("{ctrl-c}x{ret}"), ["ctrl-c", "x", "ret"]);
        assert_eq!(keys("a{b"), ["a", "shift-bracket_left", "b"]);
        assert_eq!(keys("~|:\"<>?"), ["shift-grave_accent", "shift-backslash", "shift-semicolon", "shift-apostrophe", "shift-comma", "shift-dot", "shift-slash"]);
        assert_eq!(keys("é"), Vec::<String>::new());
    }

    #[test]
    fn escapes_are_decoded_like_python_unicode_escape() {
        assert_eq!(unescape("a\\nb\\tc\\\\d\\x41"), "a\nb\tc\\dA");
        assert_eq!(unescape("trailing\\"), "trailing\\");
        assert_eq!(unescape("\\q"), "\\q");
    }
}

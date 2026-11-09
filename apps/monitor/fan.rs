//! Existing fan daemon protocol, used only on the sampler thread.
use serde_json::Value;
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};
pub fn ask(path: &Path, command: &str) -> Result<String, &'static str> {
    let mut stream = UnixStream::connect(path).map_err(|_| "fan.unavailable")?;
    let timeout = Some(Duration::from_millis(300));
    stream
        .set_read_timeout(timeout)
        .map_err(|_| "fan.unavailable")?;
    stream
        .set_write_timeout(timeout)
        .map_err(|_| "fan.unavailable")?;
    stream
        .write_all(format!("{command}\n").as_bytes())
        .map_err(|_| "fan.unavailable")?;
    let mut reply = String::new();
    stream
        .take(8193)
        .read_to_string(&mut reply)
        .map_err(|_| "fan.unavailable")?;
    if reply.len() > 8192 {
        return Err("fan.badRequest");
    }
    Ok(reply.trim().into())
}
fn request(command: &str) -> Result<String, &'static str> {
    let path = std::env::var_os("LORD_FAND_SOCK").unwrap_or_else(|| "/run/lordos/fand.sock".into());
    ask(Path::new(&path), command)
}
pub fn status() -> Value {
    request("STATUS")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}
pub fn force(on: bool) -> Result<(), String> {
    match request(if on { "FORCE 30" } else { "FORCE off" }) {
        Ok(s) if s == "OK" => Ok(()),
        Ok(s) if s.starts_with("ERR fan.") && s.len() < 40 => Err(s[4..].into()),
        Ok(_) => Err("fan.badRequest".into()),
        Err(e) => Err(e.into()),
    }
}

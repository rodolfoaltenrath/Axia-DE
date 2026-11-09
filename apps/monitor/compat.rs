//! Compatibility boundary for existing shell consumers (disk alerts, traffic item).
//! All collection, lifecycle, validation and history policy remain in Rust.
use super::worker::{wall, Job, Service, Want};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    ffi::{c_char, CStr, CString},
    sync::atomic::{AtomicU64, Ordering},
};
#[derive(Default)]
struct Client {
    want: Want,
    seq: std::collections::BTreeMap<String, u64>,
}
thread_local! {static CLIENT:RefCell<Client>=RefCell::new(Client::default());}
pub fn request(input: &Value) -> Value {
    let cmd = input["cmd"].as_str().unwrap_or("");
    let args = &input["args"];
    let service = Service::shared();
    CLIENT.with(|client|{let mut client=client.borrow_mut();match cmd{
        "reset"=>{service.legacy(false,None);*client=Client::default();json!(true)},
        "start"=>{service.legacy(true,Some(client.want.clone()));json!(true)},
        "stop"=>{service.legacy(false,None);json!(true)},
        "want"=>{client.want.processes=args[0]==true;client.want.connections=args[1]==true;client.want.geo=args[2]==true;service.legacy(true,Some(client.want.clone()));json!(true)},
        "watchDisks"=>{service.watch(args[0]==true);json!(true)},
        "wantVoltage"=>{client.want.voltage=args[0]==true;service.legacy(true,Some(client.want.clone()));json!(true)},
        "detail"=>{let pid=args[0].as_i64().unwrap_or(0);let st=args[1].as_u64().unwrap_or(0);if !(0..=4194304).contains(&pid){return json!({"error":"sys.badArgument"})}client.want.detail=(pid>0).then_some((pid as i32,st));service.legacy(true,Some(client.want.clone()));client.seq.remove("detailInfo");json!(true)},
        "signal"=>{let Some(pid)=args[0].as_i64().and_then(|p|i32::try_from(p).ok())else{return json!("sys.badArgument")};let Some(st)=args[1].as_u64()else{return json!("sys.badArgument")};super::processes::signal(pid,st,args[2].as_str().unwrap_or("")).err().map(|s|json!(s)).unwrap_or(Value::Null)},
        "historyEnable"=>{service.history(args[0]==true);json!(args[0]==true)},
        "historyClear"=>{service.job(Job::Clear);json!(true)},
        "history"=>{service.job(Job::Query(args[0].clone()));service.snapshot().history.clone()},
        "info"=>{let snapshot=service.snapshot();if snapshot.info.is_null(){json!({"cpuModel":"","cores":unsafe{libc::sysconf(libc::_SC_NPROCESSORS_ONLN)},"geo":true,"tick":250,"home":null})}else{snapshot.info.clone()}},
        "netTotals"=>{service.job(Job::Totals);service.snapshot().totals.clone()},
        "sample"|"processes"|"connections"|"voltage"|"diskTemps"|"detailInfo"=>{let snapshot=service.snapshot();let seq=client.seq.entry(cmd.into()).or_default();if *seq==snapshot.seq{return Value::Null}*seq=snapshot.seq;match cmd{
            "sample"=>snapshot.samples.back().cloned().unwrap_or(Value::Null),"processes"=>snapshot.processes.clone(),"connections"=>snapshot.connections.clone(),"voltage"=>snapshot.sensors["voltage"].clone(),"diskTemps"=>json!({"t":wall(),"disks":snapshot.samples.back().map(|s|&s["disk"]["disks"])}),_=>client.want.detail.and_then(|id|snapshot.details.get(&id).cloned()).unwrap_or(Value::Null)
        }},
        _=>Value::Null,
    }})
}
#[no_mangle]
pub unsafe extern "C" fn axia_monitor_request(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return std::ptr::null_mut();
    }
    let bytes = CStr::from_ptr(input).to_bytes();
    if bytes.len() > 1 << 20 {
        return std::ptr::null_mut();
    }
    let reply = serde_json::from_slice(bytes)
        .map(|v| request(&v))
        .unwrap_or(Value::Null);
    CString::new(reply.to_string()).unwrap().into_raw()
}
#[no_mangle]
pub unsafe extern "C" fn axia_monitor_reply_free(reply: *mut c_char) {
    if !reply.is_null() {
        drop(CString::from_raw(reply));
    }
}
#[no_mangle]
pub extern "C" fn netrec_init(allowed: i32) {
    Service::shared().allowed(allowed != 0)
}
#[no_mangle]
pub extern "C" fn netrec_shutdown() {
    Service::shared().history(false)
}
#[no_mangle]
pub extern "C" fn netrec_timeout() -> f64 {
    if Service::shared().recording() {
        5000.
    } else {
        -1.
    }
}
extern "C" {
    fn js_fw_counts(seq: *mut u64) -> *const c_char;
}
#[no_mangle]
pub unsafe extern "C" fn netrec_tick() {
    if !Service::shared().recording() {
        return;
    }
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = wall();
    if now.saturating_sub(LAST.load(Ordering::Relaxed)) < 5000 {
        return;
    }
    LAST.store(now, Ordering::Relaxed);
    let mut seq = 0;
    let ptr = js_fw_counts(&mut seq);
    if !ptr.is_null() {
        if let Ok(v) = serde_json::from_slice(CStr::from_ptr(ptr).to_bytes()) {
            Service::shared().job(Job::Counts(v));
        }
    }
}

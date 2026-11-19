//! Subprocesses used only by Rust session integration tests; not installed.
use std::{
    env, fs,
    os::unix::{net::UnixListener, process::CommandExt},
    path::PathBuf,
    process::Command,
    time::Duration,
};

fn main() {
    let name = env::args_os().next().map(PathBuf::from).unwrap();
    let name = name.file_name().unwrap().to_str().unwrap();
    let args: Vec<_> = env::args_os().skip(1).collect();
    match name {
        "dbus-run-session" => {
            assert_eq!(args[0], "--");
            let error = Command::new(&args[1])
                .args(&args[2..])
                .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/private-test-bus")
                .exec();
            panic!("{error}");
        }
        "dbus-update-activation-environment" | "notify-send" => {
            let root = PathBuf::from(env::var_os("HOME").unwrap());
            let values: std::collections::BTreeMap<_, _> = env::vars().collect();
            let record = serde_json::json!({"env":values,"args":args.iter().map(|s|s.to_string_lossy()).collect::<Vec<_>>()});
            fs::write(
                root.join(format!("{name}.json")),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
            if let Ok(ms) = env::var("AXIA_TEST_HELPER_SLEEP_MS") {
                std::thread::sleep(Duration::from_millis(ms.parse().unwrap()));
            }
            std::process::exit(
                env::var("AXIA_TEST_PUBLISH_EXIT")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            );
        }
        "xdg-user-dirs-update" => (),
        "axia-server" => {
            let runtime = PathBuf::from(env::var_os("XDG_RUNTIME_DIR").unwrap());
            let _socket = if env::var("AXIA_TEST_SOCKET").as_deref() == Ok("1") {
                Some(UnixListener::bind(runtime.join("wayland-7")).unwrap())
            } else {
                None
            };
            let values: std::collections::BTreeMap<_, _> = env::vars().collect();
            let record = serde_json::json!({"env":values,"pid":std::process::id(),"cwd":env::current_dir().unwrap(),"args":args.iter().map(|s|s.to_string_lossy()).collect::<Vec<_>>()});
            println!("test compositor stdout");
            eprintln!("test compositor stderr");
            fs::write(
                PathBuf::from(env::var_os("HOME").unwrap()).join("child.json"),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
            let milliseconds = env::var("AXIA_TEST_SLEEP_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1000);
            std::thread::sleep(Duration::from_millis(milliseconds));
            std::process::exit(
                env::var("AXIA_TEST_EXIT")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            );
        }
        _ => panic!("unexpected fixture role {name}"),
    }
}

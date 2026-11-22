use super::*;
pub(super) fn fixture() -> App {
    let mut app = App::default();
    app.section = "general".into();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    app.resources(root.join("assets").to_str().unwrap(), None);
    app.loaded = true;
    app.now = 1_800_000_000_000;
    app.data=serde_json::from_str(r#"{"admin.available":true,"system.info":{"admin":true,"fullName":"Test User","user":"test","cpu":"Test CPU","cores":4,"hostname":"axia","os":"Fedora","kernel":"6.0","arch":"x86_64","memTotalKB":1024,"memAvailableKB":512},"platform":{"standalone":false,"version":"0.8.7","screen":{"width":1280,"height":800}},
        "input.devices":[{"kind":"touchpad","name":"Test pad","width":100,"height":60}],"bt.state":{"available":true,"agent":true,"adapter":{"powered":true,"path":"adapter","name":"Test"},"devices":[{"paired":true,"connected":true,"name":"Headphones","path":"device","battery":90}]},
        "sound.state":{"connected":true,"sinks":[{"index":1,"default":true,"name":"speakers","description":"Speakers","level":0.5,"mute":false,"ports":[]}],"sources":[{"index":2,"default":true,"monitorOf":-1,"name":"mic","description":"Microphone","level":0.5,"mute":false,"ports":[]}],"inputs":[]},
        "power.state":{"hasBattery":true,"percent":80,"onAC":false,"supplies":[{"battery":true,"health":90,"cycles":100}]},"power.profiles":{"profiles":["power-saver","balanced","performance"],"active":"balanced","daemon":true},"power.buttons":{"power":"suspend","lid":"suspend","lidAC":"ignore"},
        "time.zone":"America/Sao_Paulo","time.zones":["America/Sao_Paulo","UTC"],"timedate.state":{"canNTP":true,"ntp":false,"synced":true},"zoneOffset":{"offset":-10800,"abbr":"-03"},
        "fan.status":{"canChange":true,"board":"pi5","profile":"custom","force":"off","step":1,"floor":0,"hyst":3,"temp":55,"fans":[{"pct":30,"rpm":1000,"kind":"board"}],"curve":[[55,30],[60,55],[65,100]],"custom":[[55,30],[60,55],[65,100]]},"fan.case":"argon-one-v5",
        "display.list":{"onMonitor":true,"outputs":[{"id":1,"name":"Screen","active":true,"main":true,"w":1920,"h":1080,"x":0,"y":0,"scale":100,"rotation":0,"mode":{"w":1920,"h":1080,"refresh":60000},"preferred":{"w":1920,"h":1080},"resolutions":[{"w":1920,"h":1080,"rates":[60000,75000]}]}]},"display.brightness":{"devices":[{"name":"panel","percent":50}]},"video.hardware":{"vp9":true,"av1":false,"reason":"noProfiles"},
        "users.list":{"users":[{"name":"test","full":"Test User","admin":true,"self":true},{"name":"other","full":"Other User","admin":false,"self":false}],"autologin":""},"languages":[{"id":"en","name":"English","english":"English","region":"en_US"}],"locale.available":[{"name":"en_US.UTF-8","generated":true}],"locale.get":{"system":{"LANG":"C.UTF-8"}},"keyboard.list":[{"id":"us","name":"English (US)","variants":[]}],
        "net.interfaces":[{"name":"eth0","type":"ethernet","up":true,"carrier":true,"managed":"lordos","addresses":[{"family":4,"address":"192.168.1.2","prefix":24,"scope":"global"}]}],"net.routes":[],"net.dns":{"servers":[]},"net.vpnTypes":[{"type":"ipsec","available":true},{"type":"openvpn","available":true}],"net.vpnList":[],
        "print.hasCups":true,"printers":{"printers":[{"name":"printer","info":"Printer","state":3,"accepting":true}],"default":"printer"},"printJobs":{"jobs":[]},"found":{"printers":[],"scanners":[]},"catalog":[],"registry":[],"packages.list":[],"perms.list":[],"privacy.indicators":[],"clipboard.history":[],"firmware":{"state":"available","current":1,"latest":2,"release":"latest"},"pkg.available":true,"pkg.upgrades":[],"session.canPower":true
    }"#).unwrap();
    app.theme = json!({"mode":"dark","text":"#fff","textDim":"#999","accent":"#0a84ff","control":"#333","separator":"#444","accents":{"blue":"#0a84ff","red":"#ff453a"},"wallpapers":[{"id":"wallpaper","name":"Wallpaper","category":"gradients","src":"asset:test.png"}]}).into();
    app
}

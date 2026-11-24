use super::*;

fn defaults() -> Value {
    json!({"id":"","name":"Default","scrollback":10000,"font":"JetBrains Mono","fontSize":13})
}
fn run(input: Input, name: &str) -> Result<Imported, String> {
    let defaults = defaults();
    let tr = |key: &str, params: Value| format!("{key}{}", if params == json!({}) { String::new() } else { params.to_string() });
    let font = |ps: &str| (ps == "FiraCode-Regular").then(|| "Fira Code".to_owned());
    let mut n = 0;
    let mut id = || {
        n += 1;
        format!("p{n}")
    };
    let mut env = Env { defaults: &defaults, tr: &tr, font: &font, id: &mut id };
    import(input, name, &mut env)
}
fn comp(r: f64, g: f64, b: f64) -> String {
    format!("<key>Red Component</key><real>{r}</real><key>Green Component</key><real>{g}</real><key>Blue Component</key><real>{b}</real>")
}
fn colors_xml(extra: &str) -> String {
    let mut s = String::from("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>");
    for i in 0..16 {
        s += &format!("<key>Ansi {i} Color</key><dict>{}</dict>", comp(i as f64 / 15., 0., 1.));
    }
    s += &format!("<key>Foreground Color</key><dict>{}</dict>", comp(1., 1., 1.));
    s += &format!("<key>Background Color</key><dict>{}</dict>{extra}</dict></plist>", comp(0., 0., 0.));
    s
}

#[test]
fn a_color_preset_becomes_a_scheme() {
    let out = run(Input::Text(&colors_xml("")), "Dracula.itermcolors").unwrap();
    assert!(out.profiles.is_empty());
    let s = &out.schemes[0];
    assert_eq!(s["name"], "Dracula");
    assert_eq!(s["fg"], "#ffffff");
    assert_eq!(s["bg"], "#000000");
    assert_eq!(s["ansi"][0], "#0000ff");
    assert_eq!(s["ansi"][15], "#ff00ff");
    assert_eq!(s["cursor"], "#ffffff", "no cursor color follows the foreground");
    assert_eq!(s["selection"], "#b5d5ff");
}

#[test]
fn display_p3_colors_are_converted_to_srgb() {
    let p3 = "<key>Color Space</key><string>P3</string>";
    let xml = colors_xml("").replace("<key>Foreground Color</key><dict>", &format!("<key>Foreground Color</key><dict>{p3}"));
    let fg = run(Input::Text(&xml), "x.itermcolors").unwrap().schemes[0]["fg"].clone();
    assert_eq!(fg, "#ffffff", "white stays white");
    let (r, g, b) = p3_to_srgb(1., 0., 0.);
    assert!(r > 0.99 && g < 0.01 && b < 0.01, "P3 red clips to sRGB red: {r} {g} {b}");
}

#[test]
fn a_preset_missing_an_ansi_color_is_rejected() {
    let xml = colors_xml("").replace("<key>Ansi 7 Color</key>", "<key>Other</key>");
    assert_eq!(run(Input::Text(&xml), "x.itermcolors").unwrap_err(), "terminal.iterm.no16");
}

#[test]
fn json_profiles_are_converted_with_warnings_for_what_is_missing() {
    let text = r#"{"Profiles":[{"Name":"Work","Guid":"G1","Normal Font":"FiraCode-Regular 14","Cursor Type":1,"Blinking Cursor":true,
        "Transparency":0.35,"Columns":120,"Rows":40,"Unlimited Scrollback":false,"Scrollback Lines":5000,"Option Key Sends":2,
        "Custom Directory":"Yes","Working Directory":"/Users/me/code","Badge Text":"\\(user.x)","Horizontal Spacing":1.2,
        "Background Image Location":"/x.png","Bound Hosts":["me@box","&vim","/Users/me/code"],"Silence Bell":true,
        "Triggers":[{"regex":"error","action":"BellTrigger","parameter":""},{"regex":"x","action":"Mystery"}],
        "Keyboard Map":{"0xf702-0x280000":{"Action":11,"Text":"0x1b 0x62"},"0x61-0x100000":{"Action":99}}}]}"#;
    let out = run(Input::Text(text), "work.json").unwrap();
    let p = &out.profiles[0];
    assert_eq!(p["id"], "p1");
    assert_eq!(p["name"], "Work");
    assert_eq!(p["itermGuid"], "G1");
    assert_eq!(p["font"], "Fira Code");
    assert_eq!(p["fontSize"], 14);
    assert_eq!(p["cursorStyle"], "beam");
    assert_eq!(p["cursorBlink"], true);
    assert_eq!(p["transparency"], 0.35);
    assert_eq!((p["columns"].clone(), p["rows"].clone()), (json!(120), json!(40)));
    assert_eq!(p["scrollback"], 5000);
    assert_eq!(p["altSendsEscape"], true);
    assert_eq!(p["bell"], "none");
    assert_eq!(p["workingDir"], "custom");
    assert_eq!(p["customDir"], "~/code");
    assert_eq!(p["rules"], "me@box\n~/code");
    assert_eq!(p["badge"], "\\(user.x)");
    assert_eq!(p["triggers"], json!([{"regex":"error","action":"bell","param":"","enabled":true}]));
    assert_eq!(p["keyMap"], json!([{"combo":"Alt+ArrowLeft","action":"hex","param":"0x1b 0x62"}]));
    let all = out.warnings.join("|");
    for needle in ["noAnsi", "hspacing", "bgImage", "badgeVars", "jobRules", "triggersSkipped", "keysSkipped"] {
        assert!(all.contains(&format!("terminal.iterm.{needle}")), "{needle} missing in {all}");
    }
    assert!(all.starts_with("Work: "));
}

#[test]
fn an_unknown_font_is_reported_but_the_bundled_one_is_not() {
    let a = run(Input::Text(r#"{"Name":"A","Guid":"1","Normal Font":"Menlo-Regular 12"}"#), "a.json").unwrap();
    assert!(a.warnings.iter().any(|w| w.contains("noFont")));
    let b = run(Input::Text(r#"{"Name":"B","Guid":"2","Normal Font":"JetBrainsMono-Regular 12"}"#), "b.json").unwrap();
    assert!(!b.warnings.iter().any(|w| w.contains("noFont")));
}

#[test]
fn a_custom_command_is_typed_into_the_shell() {
    let out = run(Input::Text(r#"{"Name":"S","Guid":"3","Custom Command":"Yes","Command":"ssh box","Initial Text":"ls"}"#), "s.json").unwrap();
    assert_eq!(out.profiles[0]["initialText"], "ssh box\nls");
    assert!(out.warnings.iter().any(|w| w.contains("terminal.iterm.command")));
}

#[test]
fn key_combinations_follow_the_key_tables() {
    assert_eq!(combo("0xf702-0x280000").as_deref(), Some("Alt+ArrowLeft"));
    assert_eq!(combo("0xf703-0x100000").as_deref(), Some("Super+ArrowRight"));
    assert_eq!(combo("0x74-0x100000").as_deref(), Some("Super+t"));
    assert_eq!(combo("0x54-0x100000").as_deref(), Some("Super+Shift+t"));
    assert_eq!(combo("0x7b-0x100000").as_deref(), Some("Super+Shift+["));
    assert_eq!(combo("0xf704-0x0").as_deref(), Some("F1"));
    assert_eq!(combo("0x19-0x0").as_deref(), Some("Shift+Tab"));
    assert_eq!(combo("0x40000").as_deref(), None);
    assert_eq!(combo("0x1f600-0x0").as_deref(), None);
}

#[test]
fn the_full_preferences_give_profiles_default_shortcuts_and_presets() {
    let xml = format!(
        "<plist><dict><key>Default Bookmark Guid</key><string>G2</string><key>New Bookmarks</key><array>\
         <dict><key>Name</key><string>One</string><key>Guid</key><string>G1</string></dict>\
         <dict><key>Name</key><string>Two &amp; Co</string><key>Guid</key><string>G2</string></dict></array>\
         <key>GlobalKeyMap</key><dict><key>0x74-0x100000</key><dict><key>Action</key><integer>42</integer></dict></dict>\
         <key>Custom Color Presets</key><dict><key>Mine</key><dict>{}</dict></dict></dict></plist>",
        (0..16).map(|i| format!("<key>Ansi {i} Color</key><dict>{}</dict>", comp(0.5, 0.5, 0.5))).collect::<String>()
    );
    let out = run(Input::Text(&xml), "com.googlecode.iterm2.plist").unwrap();
    assert_eq!(out.profiles.len(), 2);
    assert_eq!(out.profiles[1]["name"], "Two & Co");
    assert_eq!(out.default_guid.as_deref(), Some("G2"));
    assert_eq!(out.key_map, vec![json!({"combo":"Super+t","action":"undoClose","param":""})]);
    assert_eq!(out.schemes[0]["name"], "Mine");
}

#[test]
fn xml_entities_and_empty_elements_are_read() {
    let doc = Xml::new("<plist><dict><key>a</key><string>x &lt;&#65;&#x42;&gt;</string><key>b</key><true/><key>c</key><array/><key>d</key><integer>7</integer></dict></plist>").value(0);
    assert_eq!(doc, json!({"a":"x <AB>","b":true,"c":[],"d":7}));
}

/// A tiny bplist00 writer: strings, ints, bools, arrays and dictionaries.
fn bplist(root: &Value) -> Vec<u8> {
    fn flatten(v: &Value, objects: &mut Vec<Value>) -> usize {
        let at = objects.len();
        objects.push(Value::Null);
        let entry = match v {
            Value::Array(a) => json!({"a": a.iter().map(|x| flatten(x, objects)).collect::<Vec<_>>()}),
            Value::Object(o) => {
                let keys = o.keys().map(|k| flatten(&json!(k), objects)).collect::<Vec<_>>();
                let vals = o.values().map(|x| flatten(x, objects)).collect::<Vec<_>>();
                json!({"d": [keys, vals]})
            }
            other => json!({"v": other}),
        };
        objects[at] = entry;
        at
    }
    let mut objects = vec![];
    flatten(root, &mut objects);
    let mut out = b"bplist00".to_vec();
    let mut offsets = vec![];
    for o in &objects {
        offsets.push(out.len());
        let head = |hi: u8, n: usize, out: &mut Vec<u8>| {
            if n < 15 {
                out.push((hi << 4) | n as u8);
            } else {
                out.extend([(hi << 4) | 15, 0x10, n as u8]);
            }
        };
        if let Some(a) = o.get("a") {
            let a = a.as_array().unwrap();
            head(0xa, a.len(), &mut out);
            out.extend(a.iter().map(|r| r.as_u64().unwrap() as u8));
        } else if let Some(d) = o.get("d") {
            let (k, v) = (d[0].as_array().unwrap(), d[1].as_array().unwrap());
            head(0xd, k.len(), &mut out);
            out.extend(k.iter().chain(v).map(|r| r.as_u64().unwrap() as u8));
        } else {
            match &o["v"] {
                Value::String(s) => {
                    head(0x5, s.len(), &mut out);
                    out.extend(s.bytes());
                }
                Value::Bool(b) => out.push(if *b { 9 } else { 8 }),
                Value::Number(n) if n.is_f64() => {
                    out.push(0x23);
                    out.extend(n.as_f64().unwrap().to_be_bytes());
                }
                Value::Number(n) => {
                    out.push(0x11);
                    out.extend((n.as_u64().unwrap() as u16).to_be_bytes());
                }
                _ => out.push(0),
            }
        }
    }
    let table = out.len();
    out.extend(offsets.iter().map(|o| *o as u8));
    out.extend([0u8; 6]);
    out.extend([1, 1]);
    out.extend((objects.len() as u64).to_be_bytes());
    out.extend(0u64.to_be_bytes());
    out.extend((table as u64).to_be_bytes());
    out
}

#[test]
fn a_binary_plist_is_read_like_the_xml_one() {
    let doc = json!({"Name":"Bin","Guid":"B1","Blur":true,"Columns":100,"Rows":30,"Transparency":0.5,"Scrollback Lines":777});
    let out = run(Input::Bytes(&bplist(&doc)), "x.plist").unwrap();
    let p = &out.profiles[0];
    assert_eq!((p["name"].clone(), p["blur"].clone(), p["columns"].clone(), p["scrollback"].clone()), (json!("Bin"), json!(true), json!(100), json!(777)));
    assert_eq!(p["transparency"], 0.5);
}

#[test]
fn a_damaged_binary_plist_fails_cleanly() {
    let mut good = bplist(&json!({"Name":"Bin","Guid":"B1"}));
    // an object reference that points outside the table
    let n = good.len();
    good[n - 8..].copy_from_slice(&u64::MAX.to_be_bytes());
    assert!(run(Input::Bytes(&good), "x.plist").is_err());
    assert_eq!(run(Input::Bytes(b"bplist00short"), "x.plist").unwrap_err(), "terminal.iterm.short");
    assert_eq!(run(Input::Bytes(b"not a plist at all, nothing here at all, nothing"), "x").unwrap_err(), "terminal.iterm.unknownBinary");
}

#[test]
fn text_that_is_not_an_export_is_refused() {
    assert_eq!(run(Input::Text("hello"), "x").unwrap_err(), "terminal.iterm.notIterm");
    assert_eq!(run(Input::Text("[]"), "x").unwrap_err(), "terminal.iterm.noProfiles");
}

use super::*;
fn facts() -> Value {
    json!({"cmd":"sync","context":{"theme":{"mode":"dark","text":"#fff","textDim":"#aaa","separator":"#333"},"decimal":",","group":".","reactVersion":"19.3.1","screen":{"width":1920,"height":1080}},"services":{"system":{"info":{"os":"Test Linux","kernel":"6.18","arch":"x86_64","hostname":"test","user":"user","cpu":"Example(R) CPU Processor Rev 1.0","cores":8,"memTotalKB":8388608,"memAvailableKB":4194304,"uptime":90060,"bootSeconds":1.2345,"bootShown":"session","version":"0.8.7","channel":"dev","build":"test-build"}},"$root":{"stats":{"rssKB":10240,"jsKB":2048,"nodes":1234,"layoutMs":0.5,"paintMs":1.2}},"code":{"status":{"state":"verified","files":10,"refused":[]}},"fan":{"caseConfig":"argon-one-v5"}}})
}
#[test]
fn original_content_branding_regional_formatting_and_optional_rows() {
    let mut app = App;
    let tree = app.request(&facts())["tree"].to_string();
    for expected in [
        "Test Linux",
        "Example",
        "1,234",
        "4,0 GB",
        "1 d 1 h",
        "1,7",
        "about.bootToDesktop",
        "about.caseArgonV5",
        "about.codeVerifiedNote",
        "QuickJS + React 19.3.1",
        "1920 × 1080",
        "axia-icon-128.png",
        "about.credits",
    ] {
        assert!(tree.contains(expected), "{expected}: {tree}");
    }
    assert!(!tree.contains("Rev 1.0"));
    let mut input = facts();
    input["services"]["system"]["info"]["bootSeconds"] = json!(0);
    input["services"]["fan"]["caseConfig"] = Value::Null;
    input["services"]["code"]["status"]["refused"] = json!([{"path":"modified"}]);
    let tree = app.request(&input)["tree"].to_string();
    assert!(tree.contains("about.codeInvalid"));
    for absent in [
        "about.caseArgonV5",
        "about.bootToDesktop",
        "about.codeVerifiedNote",
    ] {
        assert!(!tree.contains(absent));
    }
}
#[test]
fn report_effect_icons_and_missing_services() {
    let mut app = App;
    assert_eq!(
        app.request(&json!({"cmd":"report"}))["effect"],
        json!({"kind":"openApp","app":"sysinfo"})
    );
    assert!(!app.request(&json!({"cmd":"sync"}))["tree"].is_null());
    let small = app.request(&json!({"cmd":"icon","size":24})).to_string();
    assert!(small.contains("axia-icon-32.png"));
    assert!(small.contains("none"));
}

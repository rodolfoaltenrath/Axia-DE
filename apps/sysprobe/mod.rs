//! The sandbox checks already live in server/src/app/probe.rs and run before
//! loading a window. This is the original in-process fallback and chip icon.
use crate::native_view::NativeApp;
use serde_json::{json, Value};
#[derive(Default)]
pub struct App;
impl NativeApp for App {
    fn request(&mut self, input: &Value) -> Value {
        match input["cmd"].as_str().unwrap_or("") {
            "icon" => icon(input["size"].as_f64().unwrap_or(32.)),
            "sync" => json!({"tree":{"kind":"view","key":"probe-fallback","style":{"flex":1,"padding":20,"justifyContent":"center"},"children":[{"kind":"text","label":{"key":"sysinfo.probe.inProcess"},"style":{"fontSize":13,"color":input["context"]["theme"]["textDim"],"textAlign":"center","lineHeight":1.4}}]}}),
            _ => Value::Null,
        }
    }
}
fn icon(s: f64) -> Value {
    let s = if s.is_finite() { s.clamp(1., 512.) } else { 32. };
    let mut children = vec![json!({"kind":"view","key":"chip","style":{"width":s*0.5,"height":s*0.5,"borderRadius":s*0.08,"border":format!("{}px solid rgba(255,255,255,0.95)",(s*0.04).max(1.)),"alignItems":"center","justifyContent":"center"},"children":[{"kind":"view","key":"core","style":{"width":s*0.22,"height":s*0.22,"borderRadius":s*0.03,"backgroundColor":"#FFD60A"}}]})];
    for (i,p) in [0.32,0.5,0.68].iter().enumerate() {
        for edge in ["top","bottom"] {
            let mut style = json!({"position":"absolute","left":s*p-s*0.02,"width":s*0.04,"height":s*0.08,"backgroundColor":"rgba(255,255,255,0.8)"});
            style[edge]=json!(s*0.16);
            children.push(json!({"kind":"view","key":format!("{edge}-{i}"),"style":style}));
        }
    }
    json!({"kind":"view","key":"probe-icon","style":{"width":s,"height":s,"borderRadius":s*0.22,"alignItems":"center","justifyContent":"center","background":"linear-gradient(135deg, #B46CFF, #4F46E5)"},"children":children})
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{native_view::Host, runtime::native as scene};
    #[test]
    fn original_fallback_and_icon_mount_without_react() {
        let mount=scene::create(1);
        let assets=concat!(env!("CARGO_MANIFEST_DIR"),"/../../assets");
        let mut host=Host::new(mount,Box::new(App),assets,None);
        host.facts(json!({"lang":"en","context":{"theme":{"textDim":"#aabbcc"}}}));
        let mut i18n=crate::native_view::i18n::I18n::new(assets,None);
        assert_eq!(scene::gather_text(mount),i18n.caption(&json!({"key":"sysinfo.probe.inProcess"})));
        host.facts(json!({"icon":48}));
        assert!(scene::gather_text(mount).is_empty());
        let tree=icon(48.);
        assert_eq!(tree["children"].as_array().unwrap().len(),7);
        assert_eq!(tree["children"][0]["style"]["width"],24.);
        assert_eq!(tree["children"][1]["style"]["top"],48.*0.16);
        drop(host);
        scene::destroy(mount);
    }
}

//! One mounted native view: the mirror of the app's widget tree in the scene,
//! keyed reconciliation, the shared widgets (button, scroll view, text field) and
//! their interaction, the refresh timer and the queue of shell effects.
use super::{
    i18n::I18n,
    style::{self, Props},
    NativeApp,
};
use crate::runtime::native as scene;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, VecDeque};
const CARET_BLINK: f64 = 530.;
const BAR_SHOWN: f64 = 1000.;
const WHEEL_STEP: f64 = 48.;
pub fn now_ms() -> f64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec as f64 * 1e3 + t.tv_nsec as f64 / 1e6
}
/// The facts a sync request carries (lent from `Host::facts`). `lang`: for strings the app picks
/// itself (localized package manifests).
const SYNC_FACTS: [&str; 7] = ["args", "settings", "services", "catalog", "windows", "registry", "lang"];
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    View,
    Text,
    Image,
    Raster,
    Icon,
    Button,
    Scroll,
    Input,
    /// App and window icons, unknown kinds and nulls render nothing (as in React).
    Empty,
}
fn kind_of(v: &Value) -> Kind {
    match v["kind"].as_str() {
        Some("view") => Kind::View,
        Some("text") => Kind::Text,
        Some("image") => Kind::Image,
        Some("raster") => Kind::Raster,
        Some("appIcon" | "windowIcon") => Kind::Icon,
        Some("button") => Kind::Button,
        Some("scroll") => Kind::Scroll,
        Some("input") => Kind::Input,
        _ => Kind::Empty,
    }
}
/// A widget as it is mounted: its nodes, the inputs they were built from and the
/// state that outlives rebuilds (a scroll offset).
struct Mounted {
    kind: Kind,
    key: String,
    node: i32,
    /// Button label; scroll content and thumb; field text, caret and placeholder.
    parts: [i32; 3],
    raw: [Value; 4],
    props: [Props; 4],
    label: String,
    /// A field's placeholder as shown.
    aux: Option<String>,
    image: (String, u64),
    spec: Value,
    /// A view's `onClick` action.
    click: Option<Value>,
    /// The shell layer's node a `"layer"` view lives in (menus, popovers: above the windows); 0 in place.
    layer: i32,
    /// A view with a `hoverStyle`: its style without it.
    base: Value,
    offset: f64,
    children: Vec<Mounted>,
}
impl Mounted {
    fn new(kind: Kind, key: String) -> Self {
        Self {
            kind,
            key,
            node: 0,
            parts: [0; 3],
            raw: Default::default(),
            props: Default::default(),
            label: String::new(),
            aux: None,
            image: (String::new(), u64::MAX),
            spec: Value::Null,
            click: None,
            layer: 0,
            base: Value::Null,
            offset: 0.,
            children: Vec::new(),
        }
    }
    /// Sets the style of `node` (0) or a part (1-3), only when its source changed.
    fn style(&mut self, slot: usize, raw: Value) {
        if self.raw[slot] == raw {
            return;
        }
        let node = if slot == 0 {
            self.node
        } else {
            self.parts[slot - 1]
        };
        let next = style::normalize(&raw);
        scene::set_style(node, &style::diff(&self.props[slot], &next));
        self.props[slot] = next;
        self.raw[slot] = raw;
    }
    fn destroy(self) {
        // Children and parts are inside `node`, except the layered ones: they hang from a shell layer.
        fn layered(list: &[Mounted]) {
            for m in list {
                if m.layer != 0 {
                    scene::destroy(m.node);
                } else {
                    layered(&m.children);
                }
            }
        }
        layered(&self.children);
        scene::destroy(self.node);
    }
}
fn find(list: &mut [Mounted], node: i32) -> Option<&mut Mounted> {
    for m in list {
        if m.node == node {
            return Some(m);
        }
        if let Some(found) = find(&mut m.children, node) {
            return Some(found);
        }
    }
    None
}
/// What a node does under the pointer, collected after each reconciliation.
#[derive(Clone)]
enum Target {
    Click(Value, Value),
    Field,
    Scroll,
    Thumb(i32),
}
/// `style` (an object, or arrays of them) with the window's origin added to its left and top.
fn offset_origin(style: Value, dx: f64, dy: f64) -> Value {
    match style {
        Value::Object(mut o) => {
            for (key, d) in [("left", dx), ("top", dy)] {
                if let Some(v) = o.get(key).and_then(Value::as_f64) {
                    o.insert(key.into(), json!(v + d));
                }
            }
            Value::Object(o)
        }
        Value::Array(list) => Value::Array(list.into_iter().map(|v| offset_origin(v, dx, dy)).collect()),
        other => other,
    }
}
pub struct Host {
    mount: i32,
    app: Box<dyn NativeApp>,
    i18n: I18n,
    assets: String,
    facts: Value,
    root: Vec<Mounted>,
    targets: HashMap<i32, Target>,
    buttons: HashMap<i32, i32>,
    /// The nodes of the layered views (see `Mounted::layer`), for `owns`.
    portals: Vec<i32>,
    effects: VecDeque<Value>,
    title: String,
    window_focused: bool,
    width: f32,
    next_sync: Option<f64>,
    hover: i32,
    pressed: i32,
    field: i32,
    caret_on: bool,
    next_blink: Option<f64>,
    bars: HashMap<i32, f64>,
    drag: Option<(i32, f64)>,
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.app.request(&json!({"cmd":"close"}));
        if !scene::alive() {
            return;
        }
        for m in self.root.drain(..) {
            // React may have destroyed the mount node (and these) first; the scene reuses
            // freed ids, so only nodes still under the mount are ours to destroy.
            if m.layer != 0 || (scene::within(m.node, self.mount) && m.node != self.mount) {
                m.destroy();
            }
        }
    }
}
impl Host {
    pub fn new(mount: i32, mut app: Box<dyn NativeApp>, assets: &str, home: Option<&str>) -> Self {
        app.resources(assets, home);
        Self {
            mount,
            app,
            i18n: I18n::new(assets, home),
            assets: assets.into(),
            facts: Value::Null,
            root: Vec::new(),
            targets: HashMap::new(),
            buttons: HashMap::new(),
            portals: Vec::new(),
            effects: VecDeque::new(),
            title: String::new(),
            window_focused: true,
            width: 0.,
            next_sync: None,
            hover: 0,
            pressed: 0,
            field: 0,
            caret_on: true,
            next_blink: None,
            bars: HashMap::new(),
            drag: None,
        }
    }
    fn theme(&self, key: &str, fallback: &str) -> Value {
        let v = &self.facts["context"]["theme"][key];
        if v.is_null() {
            json!(fallback)
        } else {
            v.clone()
        }
    }
    fn dark(&self) -> bool {
        self.facts["context"]["theme"]["mode"] != "light"
    }
    fn icon_mode(&self) -> Option<f64> {
        self.facts["icon"].as_f64()
    }
    fn refresh_ms(&self) -> Option<f64> {
        if self.icon_mode().is_some() {
            return None;
        }
        let ms = self
            .app
            .refresh_ms()
            .or_else(|| self.facts["refreshMs"].as_f64())
            .filter(|ms| *ms > 0.)?;
        let ms = ms.max(if self.app.refresh_ms().is_some() {
            16.
        } else {
            250.
        });
        Some(if self.window_focused {
            ms
        } else {
            (ms * 2.).max(2000.)
        })
    }
    pub fn facts(&mut self, facts: Value) {
        if facts == self.facts {
            return;
        }
        self.facts = facts;
        self.i18n.lang = self.facts["lang"].as_str().unwrap_or("en").to_owned();
        self.i18n.decimal = self.facts["context"]["decimal"]
            .as_str()
            .unwrap_or(".")
            .to_owned();
        if let Some(focused) = self.facts["focused"].as_bool() {
            self.change_window_focus(focused);
        }
        self.sync();
    }
    pub fn window_focus(&mut self, focused: bool) {
        if self.change_window_focus(focused) {
            self.sync();
        }
    }
    /// Apply focus cleanup without an intermediate tree. facts() will sync
    /// once after both the cleanup and the new context have reached the app.
    fn change_window_focus(&mut self, focused: bool) -> bool {
        if self.window_focused == focused {
            return false;
        }
        self.window_focused = focused;
        let mut changed = false;
        if !focused {
            self.blur();
            let pressed = std::mem::take(&mut self.pressed);
            changed |= self.pointer_action(pressed, "onMouseUp");
            changed |= self.update_hover(0);
        }
        // The refresh period depends on focus.
        self.next_sync = self.refresh_ms().map(|ms| now_ms() + ms);
        changed
    }
    pub fn pending(&self) -> bool {
        !self.effects.is_empty()
    }
    pub fn take(&mut self) -> Option<Value> {
        (!self.effects.is_empty()).then(|| Value::Array(self.effects.drain(..).collect()))
    }
    fn queue(&mut self, reply: &Value) {
        if !reply["effect"].is_null() {
            self.effects.push_back(reply["effect"].clone());
        }
        if !reply["title"].is_null() {
            let title = self.i18n.caption(&reply["title"]);
            if title != self.title {
                self.title = title.clone();
                self.effects
                    .push_back(json!({"kind":"title","title":title,"host":true}));
            }
        }
    }
    pub fn result(&mut self, result: Value) {
        let reply = self.app.request(&json!({"cmd":"next","result":result}));
        self.queue(&reply);
        self.sync();
    }
    /// Read-only caption lookup for accessibility and compositor integration probes.
    pub fn find_text(&self, label: &str) -> i32 {
        fn walk(list: &[Mounted], label: &str) -> Option<i32> {
            for m in list {
                if matches!(m.kind,Kind::Text|Kind::Button) && m.label == label {
                    return Some(m.node);
                }
                if m.kind == Kind::Input && (m.label == label || (m.label.is_empty() && m.aux.as_deref() == Some(label))) { return Some(m.node); }
                if let Some(node)=walk(&m.children,label) { return Some(node); }
            }
            None
        }
        if label.is_empty() { 0 } else { walk(&self.root,label).unwrap_or(0) }
    }
    pub fn text(&self) -> String {
        scene::gather_text(self.mount)
    }
    fn context(&mut self) -> Value {
        let width = scene::frame(self.mount)
            .map(|f| f.2)
            .filter(|w| *w > 1.)
            .unwrap_or_else(|| self.facts["width"].as_f64().unwrap_or(1000.) as f32);
        self.width = width;
        let mut context = self.facts["context"].clone();
        if !context.is_object() {
            context = json!({});
        }
        context["width"] = json!(width);
        context
    }
    /// Asks the app for its tree and reconciles it into the scene.
    fn sync(&mut self) {
        let now = now_ms();
        let reply = if let Some(size) = self.icon_mode() {
            json!({"tree": self.app.request(&json!({"cmd":"icon","size":size,"args":self.facts["args"]}))})
        } else {
            let context = self.context();
            // The facts are lent to the request, not copied: the catalog, the registry and the window
            // list were cloned whole on every sync (every second for Monitor or Clock, every event),
            // and given back after. A fact the shell didn't give stays absent (null to the app).
            let mut input = Map::new();
            input.insert("cmd".into(), json!("sync"));
            if let Value::Object(facts) = &mut self.facts {
                for k in SYNC_FACTS {
                    if let Some(v) = facts.get_mut(k) {
                        input.insert(k.into(), std::mem::take(v));
                    }
                }
            }
            input.insert("context".into(), context);
            let mut input = Value::Object(input);
            let reply = self.app.request(&input);
            if let (Value::Object(facts), Value::Object(lent)) = (&mut self.facts, &mut input) {
                for k in SYNC_FACTS {
                    if let (Some(v), Some(slot)) = (lent.remove(k), facts.get_mut(k)) {
                        *slot = v;
                    }
                }
            }
            reply
        };
        self.next_sync = self.refresh_ms().map(|ms| now + ms);
        self.queue(&reply);
        if reply["same"] == true || reply["tree"].is_null() {
            return;
        }
        let old = std::mem::take(&mut self.root);
        let tree = std::slice::from_ref(&reply["tree"]);
        self.root = self.reconcile(self.mount, old, tree);
        self.index();
    }
    fn key_of(v: &Value, i: usize) -> String {
        match &v["key"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => format!("#{i}"),
        }
    }
    fn reconcile(&mut self, parent: i32, old: Vec<Mounted>, new: &[Value]) -> Vec<Mounted> {
        let mut old: Vec<Option<Mounted>> = old.into_iter().map(Some).collect();
        let mut out = Vec::with_capacity(new.len());
        for (i, spec) in new.iter().enumerate() {
            let kind = kind_of(spec);
            if kind == Kind::Empty {
                continue;
            }
            let key = Self::key_of(spec, i);
            let reuse = old
                .iter()
                .position(|m| m.as_ref().is_some_and(|m| m.key == key && m.kind == kind))
                .and_then(|p| old[p].take());
            let mut m = match reuse {
                Some(m) => m,
                None => match self.create(kind, key) {
                    Some(m) => m,
                    None => continue,
                },
            };
            self.update(&mut m, spec);
            out.push(m);
        }
        for m in old.into_iter().flatten() {
            if m.node == self.hover {
                self.hover = 0;
            }
            m.destroy();
        }
        // Order the nodes like the specs, moving only those out of place.
        let mut next = 0;
        for m in out.iter().rev() {
            if m.layer != 0 {
                if scene::parent(m.node) != m.layer {
                    scene::insert(m.layer, m.node, 0);
                }
                continue;
            }
            if scene::parent(m.node) != parent || scene::next_sibling(m.node) != next {
                scene::insert(parent, m.node, next);
            }
            next = m.node;
        }
        out
    }
    fn create(&mut self, kind: Kind, key: String) -> Option<Mounted> {
        let mut m = Mounted::new(kind, key);
        let node_kind = match kind {
            Kind::Text => scene::TEXT,
            Kind::Image | Kind::Raster => scene::IMAGE,
            _ => scene::VIEW,
        };
        m.node = scene::create(node_kind);
        if m.node == 0 {
            return None;
        }
        let parts: &[i32] = match kind {
            Kind::Button => &[scene::TEXT],
            Kind::Scroll => &[scene::VIEW, scene::VIEW],
            Kind::Input => &[scene::TEXT, scene::VIEW, scene::TEXT],
            _ => &[],
        };
        for (i, k) in parts.iter().enumerate() {
            let part = scene::create(*k);
            if part == 0 {
                m.destroy();
                return None;
            }
            scene::insert(m.node, part, 0);
            m.parts[i] = part;
        }
        Some(m)
    }
    fn update(&mut self, m: &mut Mounted, spec: &Value) {
        let style = spec["style"].clone();
        match m.kind {
            Kind::View => {
                // "layer": "popover" puts the view in that shell layer when the shell named its node
                // (facts.layers); without one it stays where it is, an absolute view of the window.
                m.layer = spec["layer"]
                    .as_str()
                    .and_then(|name| self.facts["layers"][name].as_i64())
                    .map_or(0, |n| n as i32);
                if m.layer != 0 && !scene::exists(m.layer) {
                    m.layer = 0;
                }
                // "anchor": "mount": left and top are the window's, not the screen's (a menu opened at
                // a point of the view, in a layer that covers the screen).
                let style = match (m.layer, spec["anchor"].as_str(), scene::frame(self.mount)) {
                    (l, Some("mount"), Some((fx, fy, _, _))) if l != 0 => offset_origin(style, fx as f64, fy as f64),
                    _ => style,
                };
                // "hoverStyle": merged over the style while the pointer is on the view.
                let hover = spec.get("hoverStyle").filter(|h| h.is_object()).cloned();
                m.base = if hover.is_some() { style.clone() } else { Value::Null };
                m.style(0, match &hover {
                    Some(h) if self.hover == m.node => json!([style, h]),
                    _ => style,
                });
                // Views only need their event map (and hover style) after reconciliation. Avoid
                // retaining another copy of the entire descendant tree.
                let mut keep = Map::new();
                if let Some(events) = spec.get("events") {
                    keep.insert("events".into(), events.clone());
                }
                if let Some(h) = hover {
                    keep.insert("hover".into(), h);
                }
                m.spec = if keep.is_empty() { Value::Null } else { Value::Object(keep) };
                m.click = spec["events"]["onClick"]
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .map(|a| Value::Array(a.clone()));
                let old = std::mem::take(&mut m.children);
                let children = spec["children"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                m.children = self.reconcile(m.node, old, children);
            }
            Kind::Text => {
                m.style(0, style);
                let label = self.i18n.caption(&spec["label"]);
                if label != m.label {
                    scene::set_label(m.node, &label);
                    m.label = label;
                }
            }
            Kind::Image => {
                m.style(0, style);
                let src = spec["src"].as_str().unwrap_or("").to_owned();
                if src != m.image.0 {
                    if let Some(path) = self.resolve(&src) {
                        scene::set_image_path(m.node, &path);
                    }
                    m.image = (src, 0);
                }
            }
            Kind::Raster => {
                m.style(0, style);
                let id = spec["image"].as_str().unwrap_or("").to_owned();
                let generation = spec["generation"].as_u64().unwrap_or(0);
                if (id.as_str(), generation) != (m.image.0.as_str(), m.image.1) {
                    if let Some((w, h, data)) = self.app.pixels(&id) {
                        scene::set_image_pixels(m.node, w, h, data);
                    }
                    m.image = (id, generation);
                }
            }
            Kind::Icon => {
                let tree = super::icons::tree(spec, &self.facts);
                m.style(0, json!({"width":tree["style"]["width"],"height":tree["style"]["height"],"flexShrink":0}));
                let old = std::mem::take(&mut m.children);
                m.children = self.reconcile(m.node, old, std::slice::from_ref(&tree));
            }
            Kind::Button => {
                m.spec = spec.clone();
                self.button_style(m);
                let label = self.i18n.caption(&spec["label"]);
                if label != m.label {
                    scene::set_label(m.parts[0], &label);
                    m.label = label;
                }
            }
            Kind::Scroll => {
                m.style(
                    0,
                    json!([{"overflow":"hidden","flex":1,"position":"relative"}, style]),
                );
                m.style(1, json!({"translateY": -m.offset}));
                let old = std::mem::take(&mut m.children);
                let children = spec["children"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                // Children go into the content; the thumb, created after it, stays on top.
                m.children = self.reconcile(m.parts[0], old, children);
                self.thumb(m);
            }
            Kind::Input => {
                m.spec = spec.clone();
                self.field_style(m);
            }
            Kind::Empty => {}
        }
    }
    fn resolve(&self, src: &str) -> Option<String> {
        if src.starts_with('/') {
            Some(src.into())
        } else if let Some(rest) = src.strip_prefix("asset:") {
            (!rest.contains("..")).then(|| format!("{}/{}", self.assets, rest))
        } else {
            None
        }
    }
    fn button_style(&mut self, m: &mut Mounted) {
        let spec = &m.spec;
        let disabled = spec["disabled"] == true;
        let hover = self.hover == m.node && !disabled;
        let pressed = self.pressed == m.node && hover;
        let small = spec["small"] == true;
        let base = json!({
            "height":if small {22} else {28},"paddingHorizontal":if small {10} else {14},
            "borderRadius":self.theme(if small {"controlRadiusSmall"} else {"controlRadius"},"6"),
            "borderWidth":if self.facts["context"]["theme"]["buttonBorder"] == true {1} else {0},
            "borderColor":self.theme("separator","#555555"),
            "flexDirection":"row","gap":6,"justifyContent":"center","alignItems":"center",
            "backgroundColor":self.theme("control","#3a3a3f"),
            "opacity": if disabled {0.4} else if pressed {0.7} else if hover {0.85} else {1.},
        });
        let color = if spec["destructive"] == true {
            json!("#ff453a")
        } else {
            self.theme("text", "#ffffff")
        };
        m.style(0, json!([base, spec["style"]]));
        m.style(
            1,
            json!({"fontSize":if small {12} else {13},"fontWeight":500,"color":color,"whiteSpace":"nowrap"}),
        );
    }
    fn field_style(&mut self, m: &mut Mounted) {
        let focused = self.field == m.node && m.node != 0;
        let value = self.i18n.caption(&m.spec["value"]);
        let border = if focused {
            self.theme("accent", "#0a84ff")
        } else {
            self.theme("separator", "#555555")
        };
        let base = json!({
            "height":28,"paddingHorizontal":10,"borderRadius":self.theme("controlRadius","6"),
            "flexDirection":"row","alignItems":"center","overflow":"hidden","cursor":"text",
            "backgroundColor":self.theme("inputBg", if self.dark() {"#1e1e22"} else {"#ffffff"}),
            "borderWidth":1,"borderColor":border,
        });
        m.style(0, json!([base, m.spec["style"]]));
        m.style(
            1,
            json!({"fontSize":13,"color":self.theme("text","#ffffff"),"whiteSpace":"nowrap",
                "display": if value.is_empty() {"none"} else {"flex"}}),
        );
        let caret = if focused && self.caret_on {
            self.theme("accent", "#0a84ff")
        } else {
            json!("transparent")
        };
        m.style(
            2,
            json!({"width":1.5,"height":16,"marginLeft":1,"backgroundColor":caret,
                "display": if focused {"flex"} else {"none"}}),
        );
        m.style(
            3,
            json!({"fontSize":13,"color":self.theme("textDim","#999999"),"whiteSpace":"nowrap",
                "display": if value.is_empty() {"flex"} else {"none"}}),
        );
        if value != m.label {
            scene::set_label(m.parts[0], &value);
            m.label = value;
        }
        let placeholder = self.i18n.caption(&m.spec["placeholder"]);
        if m.aux.as_deref() != Some(placeholder.as_str()) {
            scene::set_label(m.parts[2], &placeholder);
            m.aux = Some(placeholder);
        }
    }
    /// Thumb size and position from the last layout; hidden without overflow.
    fn thumb(&mut self, m: &mut Mounted) {
        let view = scene::frame(m.node).map_or(0., |f| f.3 as f64);
        let content = scene::frame(m.parts[0]).map_or(0., |f| f.3 as f64);
        let shown = self.bars.contains_key(&m.node) || self.drag.is_some_and(|d| d.0 == m.node);
        let style = if content > view + 0.5 && view > 0. {
            let size = (view * view / content).max(28.).min(view);
            let top = m.offset / (content - view) * (view - size);
            let color = if self.dark() {
                "rgba(255,255,255,0.32)"
            } else {
                "rgba(0,0,0,0.3)"
            };
            json!({"position":"absolute","right":2,"top":top,"width":7,"height":size,
                "borderRadius":3.5,"backgroundColor":color,"opacity": if shown {1} else {0}})
        } else {
            json!({"position":"absolute","display":"none"})
        };
        m.style(2, style);
    }
    /// `targets`: what each interactive node does; `buttons`: hover owners.
    /// A press at `target`: the layered views it is outside of (a menu, a popover) get their
    /// `onDismiss`. True if one was sent (the caller syncs).
    fn dismiss(&mut self, target: i32) -> bool {
        let mut actions = Vec::new();
        for p in self.portals.clone() {
            if scene::within(target, p) {
                continue;
            }
            if let Some(m) = find(&mut self.root, p) {
                let action = m.spec["events"]["onDismiss"].clone();
                if action.is_array() {
                    actions.push(action);
                }
            }
        }
        for action in &actions {
            self.send(action, Map::new());
        }
        !actions.is_empty()
    }
    /// Is `target` in this view: under its mount, or in one of its layered views.
    fn owns(&self, target: i32) -> bool {
        scene::within(target, self.mount) || self.portals.iter().any(|p| scene::within(target, *p))
    }
    fn index(&mut self) {
        fn layered(list: &[Mounted], out: &mut Vec<i32>) {
            for m in list {
                if m.layer != 0 {
                    out.push(m.node);
                } else {
                    layered(&m.children, out);
                }
            }
        }
        self.portals.clear();
        layered(&self.root, &mut self.portals);
        fn walk(list: &[Mounted], t: &mut HashMap<i32, Target>, b: &mut HashMap<i32, i32>) {
            for m in list {
                match m.kind {
                    Kind::View => {
                        if let Some(action) = &m.click {
                            t.insert(m.node, Target::Click(action.clone(), Value::Null));
                        }
                        if m.spec["hover"].is_object()
                            || ["onMouseEnter", "onMouseLeave", "onMouseDown", "onMouseUp"]
                                .iter()
                                .any(|event| m.spec["events"][*event].is_array())
                        {
                            b.insert(m.node, m.node);
                        }
                    }
                    Kind::Button if m.spec["disabled"] != true => {
                        let value = m.spec.get("value").cloned().unwrap_or(Value::Null);
                        t.insert(m.node, Target::Click(m.spec["action"].clone(), value));
                        b.insert(m.node, m.node);
                    }
                    Kind::Input => {
                        t.insert(m.node, Target::Field);
                    }
                    Kind::Scroll => {
                        t.insert(m.node, Target::Scroll);
                        t.insert(m.parts[1], Target::Thumb(m.node));
                    }
                    _ => {}
                }
                walk(&m.children, t, b);
            }
        }
        // Keep the buckets across reconciliations: Calculator has the same
        // keys on every hover/press, and Monitor repeatedly rebuilds its tabs.
        self.targets.clear();
        self.buttons.clear();
        walk(&self.root, &mut self.targets, &mut self.buttons);
    }
}
/// Interaction: pointer, wheel, keys, the refresh timer and caret/scroll-bar timers.
impl Host {
    /// The nearest interactive ancestor of `node` inside this view.
    fn target(&self, mut node: i32, want: impl Fn(&Target) -> bool) -> Option<(i32, Target)> {
        for _ in 0..4096 {
            if node <= 0 || node == self.mount {
                return None;
            }
            if let Some(t) = self.targets.get(&node).filter(|t| want(t)) {
                return Some((node, t.clone()));
            }
            node = scene::parent(node);
        }
        None
    }
    /// Runs `f` on the widget mounted at `node`, lent out of the tree so `f` can
    /// use the host (theme, focus) while it restyles the widget.
    fn with_mounted(&mut self, node: i32, f: impl FnOnce(&mut Self, &mut Mounted)) {
        let Some(slot) = find(&mut self.root, node) else {
            return;
        };
        let mut m = std::mem::replace(slot, Mounted::new(Kind::Empty, String::new()));
        f(self, &mut m);
        // The lent slot still holds the placeholder (node 0) until it is put back.
        if let Some(slot) = find(&mut self.root, 0) {
            *slot = m;
        }
    }
    fn restyle(&mut self, node: i32) {
        self.with_mounted(node, |host, m| match m.kind {
            Kind::Button => host.button_style(m),
            Kind::Input => host.field_style(m),
            Kind::Scroll => host.thumb(m),
            Kind::View if m.spec["hover"].is_object() => {
                let shown = if host.hover == m.node { json!([m.base, m.spec["hover"]]) } else { m.base.clone() };
                m.style(0, shown);
            }
            _ => {}
        });
    }
    fn set_hover(&mut self, node: i32) {
        if self.update_hover(node) {
            self.sync();
        }
    }
    fn update_hover(&mut self, node: i32) -> bool {
        if node == self.hover {
            return false;
        }
        let old = std::mem::replace(&mut self.hover, node);
        let mut changed = self.pointer_action(old, "onMouseLeave");
        changed |= self.pointer_action(node, "onMouseEnter");
        for n in [old, node] {
            if n != 0 {
                self.restyle(n);
            }
        }
        changed
    }
    /// Send the action, but reconcile only after all actions of this event.
    fn pointer_action(&mut self, node: i32, event: &str) -> bool {
        if node == 0 {
            return false;
        }
        let action = find(&mut self.root, node)
            .map(|m| m.spec["events"][event].clone())
            .filter(Value::is_array);
        if let Some(action) = action {
            self.send(&action, Map::new());
            true
        } else {
            false
        }
    }
    fn focus(&mut self, node: i32) {
        if node == self.field {
            return;
        }
        let old = std::mem::replace(&mut self.field, node);
        self.caret_on = true;
        self.next_blink = (node != 0).then(|| now_ms() + CARET_BLINK);
        for n in [old, node] {
            if n != 0 {
                self.restyle(n);
            }
        }
    }
    fn blur(&mut self) {
        self.focus(0);
    }
    /// Runs a widget action like the React host's `send([cmd, key], extra)`.
    fn dispatch(&mut self, action: &Value, extra: Map<String, Value>) {
        self.send(action, extra);
        self.sync();
    }
    fn send(&mut self, action: &Value, extra: Map<String, Value>) {
        let mut input = Map::new();
        input.insert("cmd".into(), action[0].clone());
        input.insert("key".into(), action[1].clone());
        input.extend(extra);
        let reply = self.app.request(&Value::Object(input));
        self.queue(&reply);
    }
    fn scroll_by(&mut self, node: i32, delta: f64) {
        let Some(m) = find(&mut self.root, node) else {
            return;
        };
        let view = scene::frame(m.node).map_or(0., |f| f.3 as f64);
        let content = scene::frame(m.parts[0]).map_or(0., |f| f.3 as f64);
        let max = (content - view).max(0.);
        let offset = (m.offset.min(max) + delta).clamp(0., max);
        if max <= 0. {
            return;
        }
        m.offset = offset;
        m.style(1, json!({"translateY": -offset}));
        self.bars.insert(node, now_ms() + BAR_SHOWN);
        self.restyle(node);
    }
    /// The scroll offset that puts the thumb's top at `top` (drag).
    fn scroll_to_thumb(&mut self, node: i32, y: f64) {
        let Some(m) = find(&mut self.root, node) else {
            return;
        };
        let Some((_, oy, _, view)) = scene::frame(m.node) else {
            return;
        };
        let content = scene::frame(m.parts[0]).map_or(0., |f| f.3 as f64);
        let view = view as f64;
        if content <= view {
            return;
        }
        let size = (view * view / content).max(28.).min(view);
        let grab = self.drag.map_or(0., |d| d.1);
        let top = (y - oy as f64 - grab).clamp(0., view - size);
        let target = top / (view - size).max(1.) * (content - view);
        let current = m.offset;
        self.scroll_by(node, target - current);
    }
    /// Some(mount) when the event is this view's: JS then sees it on the mount node.
    pub fn pointer(&mut self, kind: &str, target: i32, x: f32, y: f32, button: i32) -> Option<i32> {
        let _ = x;
        let inside = self.owns(target);
        if self.icon_mode().is_some() {
            return inside.then_some(self.mount);
        }
        match kind {
            "move" => {
                if let Some((node, _)) = self.drag {
                    self.scroll_to_thumb(node, y as f64);
                }
                let hover = if inside {
                    let mut node = target;
                    while node != 0 && node != self.mount && !self.buttons.contains_key(&node) {
                        node = scene::parent(node);
                    }
                    if self.buttons.contains_key(&node) {
                        node
                    } else {
                        0
                    }
                } else {
                    0
                };
                self.set_hover(hover);
            }
            "down" => {
                if !self.portals.is_empty() && self.dismiss(target) {
                    self.sync();
                }
                if button != 0 {
                    let field = if inside {
                        self.target(target, |t| matches!(t, Target::Field))
                            .map_or(0, |(n, _)| n)
                    } else {
                        0
                    };
                    self.focus(field);
                    return inside.then_some(self.mount);
                }
                let old = std::mem::take(&mut self.pressed);
                if old != 0 {
                    self.restyle(old);
                }
                let mut changed = self.pointer_action(old, "onMouseUp");
                if !inside {
                    self.blur();
                    if changed {
                        self.sync();
                    }
                    return None;
                }
                if let Some((thumb, Target::Thumb(scroll))) =
                    self.target(target, |t| matches!(t, Target::Thumb(_)))
                {
                    let top = scene::frame(thumb).map_or(0., |f| f.1 as f64);
                    self.drag = Some((scroll, y as f64 - top));
                    self.bars.insert(scroll, f64::INFINITY);
                    if changed {
                        self.sync();
                    }
                    return Some(self.mount);
                }
                self.pressed = self
                    .target(target, |t| matches!(t, Target::Click(..)))
                    .map_or(0, |(n, _)| n);
                if self.pressed != 0 {
                    self.restyle(self.pressed);
                }
                changed |= self.pointer_action(self.pressed, "onMouseDown");
                let field = self
                    .target(target, |t| matches!(t, Target::Field))
                    .map_or(0, |(n, _)| n);
                self.focus(field);
                if changed {
                    self.sync();
                }
            }
            "up" => {
                if button != 0 {
                    return inside.then_some(self.mount);
                }
                if let Some((node, _)) = self.drag.take() {
                    self.bars.insert(node, now_ms() + BAR_SHOWN);
                    self.restyle(node);
                }
                let pressed = std::mem::take(&mut self.pressed);
                if pressed != 0 {
                    self.restyle(pressed);
                }
                let mut changed = self.pointer_action(pressed, "onMouseUp");
                if inside && pressed != 0 {
                    if let Some((node, Target::Click(action, value))) =
                        self.target(target, |t| matches!(t, Target::Click(..)))
                    {
                        if node == pressed {
                            let mut extra = Map::new();
                            if !value.is_null() {
                                extra.insert("value".into(), value);
                            }
                            self.send(&action, extra);
                            changed = true;
                        }
                    }
                }
                if changed {
                    self.sync();
                }
            }
            "wheel" if inside => {
                if let Some((node, _)) = self.target(target, |t| matches!(t, Target::Scroll)) {
                    self.scroll_by(node, -(button as f64) * WHEEL_STEP);
                }
            }
            _ => {}
        }
        inside.then_some(self.mount)
    }
    pub fn gesture(
        &mut self,
        kind: &str,
        phase: &str,
        target: i32,
        _dx: f64,
        dy: f64,
    ) -> Option<i32> {
        if !self.owns(target) {
            return None;
        }
        if kind == "scroll" && phase != "begin" && self.icon_mode().is_none() {
            if let Some((node, _)) = self.target(target, |t| matches!(t, Target::Scroll)) {
                self.scroll_by(node, dy);
            }
        }
        Some(self.mount)
    }
    fn edit(&mut self, text: String) {
        let Some(m) = find(&mut self.root, self.field) else {
            return;
        };
        let change = m.spec["change"].clone();
        let (action, extra) = if change.is_array() {
            (change, Map::from_iter([("value".into(), json!(text))]))
        } else {
            (json!(["query", text]), Map::new())
        };
        self.caret_on = true;
        self.next_blink = Some(now_ms() + CARET_BLINK);
        self.dispatch(&action, extra);
    }
    fn field_value(&mut self) -> Option<String> {
        let m = find(&mut self.root, self.field)?;
        Some(m.label.clone())
    }
    pub fn owns_text_input(&self) -> bool {
        self.field != 0 && self.window_focused && self.icon_mode().is_none()
    }
    /// Keys for the focused field. Shortcuts with Ctrl, Alt or Super pass, except paste.
    pub fn key(&mut self, key: &str, chr: &str, flags: i32) -> bool {
        if !self.window_focused || self.icon_mode().is_some() {
            return false;
        }
        if self.field == 0 {
            let Some(reply) = self.app.key(key, chr, flags) else {
                return false;
            };
            self.queue(&reply);
            self.sync();
            return true;
        }
        let (ctrl, alt, meta) = (flags & 2 != 0, flags & 4 != 0, flags & 16 != 0);
        let Some(value) = self.field_value() else {
            self.field = 0;
            return false;
        };
        if (ctrl || meta) && key.eq_ignore_ascii_case("v") {
            self.effects.push_back(json!({"kind":"paste","host":true}));
            return true;
        }
        match key {
            "Backspace" => {
                let mut v = value;
                v.pop();
                self.edit(v);
            }
            "Escape" => {
                let cancel = find(&mut self.root, self.field)
                    .map(|m| m.spec["cancel"].clone())
                    .filter(Value::is_array);
                self.blur();
                if let Some(action) = cancel {
                    self.dispatch(&action, Map::new());
                }
            }
            "Enter" => {
                let submit = find(&mut self.root, self.field)
                    .map(|m| m.spec["submit"].clone())
                    .filter(Value::is_array);
                if let Some(action) = submit {
                    self.blur();
                    self.dispatch(&action, Map::new());
                }
            }
            "Delete" | "ArrowLeft" | "ArrowRight" | "Home" | "End" => {}
            _ if !chr.is_empty() && !ctrl && !meta && (!alt || flags & 8 != 0) => {
                let mut v = value;
                v.push_str(&chr.replace('\n', " "));
                if v.chars().count() <= 2048 {
                    self.edit(v);
                }
            }
            _ => return false,
        }
        true
    }
    pub fn paste(&mut self, text: &str) {
        if let Some(mut value) = self.field_value() {
            value.push_str(&text.replace(['\n', '\r'], " "));
            self.edit(value.chars().take(2048).collect());
        }
    }
    pub fn tick(&mut self, now: f64) {
        // Icons depend on the explicit size in facts(), not the mount's
        // layout. Their sync path never measures context/width; checking it
        // here would rebuild every icon on every compositor frame.
        if self.icon_mode().is_some() {
            return;
        }
        // The window was resized: the app lays its tree out for the new width.
        let width = scene::frame(self.mount).map_or(0., |f| f.2);
        let resized = width > 1. && (width - self.width).abs() >= 1.;
        if resized || self.next_sync.is_some_and(|t| now >= t) {
            self.sync();
        }
        if self.next_blink.is_some_and(|t| now >= t) && self.field != 0 {
            self.caret_on = !self.caret_on;
            self.next_blink = Some(now + CARET_BLINK);
            self.restyle(self.field);
        }
        let hidden: Vec<i32> = self
            .bars
            .iter()
            .filter(|(_, until)| now >= **until)
            .map(|(n, _)| *n)
            .collect();
        for node in hidden {
            self.bars.remove(&node);
            self.restyle(node);
        }
    }
    pub fn deadline(&self) -> Option<f64> {
        [
            self.next_sync,
            self.next_blink.filter(|_| self.field != 0),
            self.bars
                .values()
                .copied()
                .filter(|t| t.is_finite())
                .reduce(f64::min),
        ]
        .into_iter()
        .flatten()
        .reduce(f64::min)
    }
}

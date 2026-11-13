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
/// How long the pointer rests on a view with a "tooltip" before the bubble shows.
const TIP_DELAY: f64 = 550.;
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
    parts: [i32; 6],
    raw: [Value; 7],
    props: [Props; 7],
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
    /// Its "tooltip" as text (a view or a text).
    tip: Option<String>,
    /// A field's texts as shown: before the selection, the selection, after it.
    parts_text: [String; 3],
    offset: f64,
    children: Vec<Mounted>,
}
impl Mounted {
    fn new(kind: Kind, key: String) -> Self {
        Self {
            kind,
            key,
            node: 0,
            parts: [0; 6],
            raw: Default::default(),
            props: Default::default(),
            label: String::new(),
            aux: None,
            image: (String::new(), u64::MAX),
            spec: Value::Null,
            click: None,
            layer: 0,
            base: Value::Null,
            tip: None,
            parts_text: Default::default(),
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
/// The texts under a hovered view (or not any more) with a `hoverStyle` take it (or leave it).
fn lit_children(list: &mut [Mounted], lit: bool) {
    for c in list {
        if c.kind == Kind::Text && c.spec["hover"].is_object() {
            let shown = if lit { json!([c.base, c.spec["hover"]]) } else { c.base.clone() };
            c.style(0, shown);
        } else if c.kind == Kind::View && c.layer == 0 && !c.spec["hover"].is_object() {
            lit_children(&mut c.children, lit);
        }
    }
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
    /// While a view that is hovered is being updated: its texts with a `hoverStyle` take it.
    lit: bool,
    /// When the bubble of the hovered view shows, and the bubble while it does.
    tip_due: Option<f64>,
    tip_view: Vec<Mounted>,
    /// The focused field's caret and the other end of its selection (in characters), and the value
    /// they were set for: another value (the app's own change) puts the caret at its end.
    /// The events of the views (node → their `events`), for the ones that bubble up to an ancestor.
    events: HashMap<i32, Value>,
    /// While a pointer event is handled: where, which button and the modifiers, sent with its actions.
    pointer_info: Option<Map<String, Value>>,
    /// The view and time of the last left click, for `onDoubleClick`.
    last_click: (i32, f64),
    /// The app asked for the pointer (a drag in progress).
    capture: bool,
    caret: usize,
    anchor: usize,
    caret_value: String,
    /// A field that asked for the focus (autoFocus) when it was mounted.
    pending_focus: Option<i32>,
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
        for m in self.tip_view.drain(..) {
            m.destroy();
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
            lit: false,
            tip_due: None,
            tip_view: Vec::new(),
            events: HashMap::new(),
            pointer_info: None,
            last_click: (0, 0.),
            capture: false,
            caret: 0,
            anchor: 0,
            caret_value: String::new(),
            pending_focus: None,
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
        for effect in reply["effects"].as_array().into_iter().flatten() {
            self.effects.push_back(effect.clone());
        }
        // "capture": the app follows the pointer wherever it goes (a drag): pointerMove on each move and
        // dragEnd when the button is released, as the React host did with global listeners.
        if let Some(on) = reply["capture"].as_bool() {
            self.capture = on;
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
        context["height"] = json!(scene::frame(self.mount).map_or(0., |f| f.3));
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
        if let Some(node) = self.pending_focus.take() {
            self.focus(node);
        }
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
            // a "menu" is drawn by views (native_view/menu.rs)
            let expanded;
            let spec = if spec["kind"] == "menu" {
                expanded = self.expand_menu(spec);
                &expanded
            } else {
                spec
            };
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
                self.hide_tip();
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
            // before the selection, a caret, the selection, a caret, after it, the placeholder
            Kind::Input => &[scene::TEXT, scene::VIEW, scene::TEXT, scene::VIEW, scene::TEXT, scene::TEXT],
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
        m.tip = match m.kind {
            Kind::View | Kind::Text if !spec["tooltip"].is_null() => Some(self.i18n.caption(&spec["tooltip"])).filter(|t| !t.is_empty()),
            _ => None,
        };
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
                let was = self.lit;
                self.lit |= self.hover == m.node && m.spec["hover"].is_object();
                m.children = self.reconcile(m.node, old, children);
                self.lit = was;
            }
            Kind::Text => {
                // "hoverStyle": taken while the view it is in is hovered (a menu row's text turns white)
                let hover = spec.get("hoverStyle").filter(|h| h.is_object()).cloned();
                m.base = if hover.is_some() { style.clone() } else { Value::Null };
                m.spec = hover.as_ref().map_or(Value::Null, |h| json!({"hover": h}));
                m.style(0, match &hover {
                    Some(h) if self.lit => json!([style, h]),
                    _ => style,
                });
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
                let first = m.aux.is_none();
                m.spec = spec.clone();
                self.field_style(m);
                if first && spec["autoFocus"] == true {
                    self.pending_focus = Some(m.node);
                }
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
        if focused && value != self.caret_value {
            // another value than the one the caret is for (the app changed it): at its end
            self.caret = value.chars().count();
            self.anchor = self.caret;
            self.caret_value = value.clone();
        }
        let chars: Vec<char> = value.chars().collect();
        let n = chars.len();
        let (caret, anchor) = if focused { (self.caret.min(n), self.anchor.min(n)) } else { (n, n) };
        let (a, b) = (caret.min(anchor), caret.max(anchor));
        let (before, sel, after): (String, String, String) = if focused {
            (chars[..a].iter().collect(), chars[a..b].iter().collect(), chars[b..].iter().collect())
        } else {
            (value.clone(), String::new(), String::new())
        };
        let accent = self.theme("accent", "#0a84ff");
        let border = if focused { accent.clone() } else { self.theme("separator", "#555555") };
        let base = json!({
            "height":28,"paddingHorizontal":10,"borderRadius":self.theme("controlRadius","6"),
            "flexDirection":"row","alignItems":"center","overflow":"hidden","cursor":"text",
            "backgroundColor":self.theme("inputBg", if self.dark() {"#1e1e22"} else {"#ffffff"}),
            "borderWidth":1,"borderColor":border,
        });
        m.style(0, json!([base, m.spec["style"]]));
        let text = self.theme("text", "#ffffff");
        let shown = |on: bool| if on { "flex" } else { "none" };
        let plain = |on: bool| json!({"fontSize":13,"color":text,"whiteSpace":"nowrap","display":shown(on)});
        let blink = if self.caret_on { accent.clone() } else { json!("transparent") };
        let caret_node = |on: bool| json!({"width":1.5,"height":16,"marginLeft":1,"backgroundColor":blink,"display":shown(on)});
        m.style(1, plain(!before.is_empty()));
        m.style(2, caret_node(focused && caret == a));
        m.style(3, json!({"fontSize":13,"color":"#ffffff","backgroundColor":accent,"whiteSpace":"nowrap","display":shown(!sel.is_empty())}));
        m.style(4, caret_node(focused && b > a && caret == b));
        m.style(5, plain(!after.is_empty()));
        m.style(6, json!({"fontSize":13,"color":self.theme("textDim","#999999"),"whiteSpace":"nowrap","display":shown(value.is_empty())}));
        for (slot, text) in [(0, before), (1, sel), (2, after)] {
            if m.parts_text[slot] != text {
                scene::set_label(m.parts[slot * 2], &text);
                m.parts_text[slot] = text;
            }
        }
        m.label = value;
        let placeholder = self.i18n.caption(&m.spec["placeholder"]);
        if m.aux.as_deref() != Some(placeholder.as_str()) {
            scene::set_label(m.parts[5], &placeholder);
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
    /// The captions of a menu's items (and their submenus') as text.
    fn resolve_items(&mut self, items: &mut Value) {
        let Some(list) = items.as_array_mut() else { return };
        for item in list {
            for key in ["label", "hint", "header"] {
                if !item[key].is_null() {
                    item[key] = Value::String(self.i18n.caption(&item[key]));
                }
            }
            if item["submenu"].is_array() {
                self.resolve_items(&mut item["submenu"]);
            }
        }
    }
    fn expand_menu(&mut self, spec: &Value) -> Value {
        let mut spec = spec.clone();
        self.resolve_items(&mut spec["items"]);
        let origin = scene::frame(self.mount).map_or((0., 0.), |f| (f64::from(f.0), f64::from(f.1)));
        // the layer covers the screen
        let screen = self.facts["layers"]["popover"]
            .as_i64()
            .and_then(|n| scene::frame(n as i32))
            .filter(|f| f.2 > 0. && f.3 > 0.)
            .map_or((1920., 1080.), |f| (f64::from(f.2), f64::from(f.3)));
        let caption = |v: &Value| v.as_str().unwrap_or("").to_owned();
        let theme = |k: &str| Some(self.facts["context"]["theme"][k].clone()).filter(|v| !v.is_null());
        let measure = |t: &str, size: f64, weight: f64| super::menu::measure(t, size, weight);
        super::menu::expand(
            &spec,
            &super::menu::Env {
                caption: &caption,
                theme: &theme,
                measure: &measure,
                screen,
                origin,
                rtl: matches!(self.i18n.lang.as_str(), "ar" | "he" | "fa" | "ur"),
                dark: self.dark(),
            },
        )
    }
    /// The pointer came to `node` (0: left every view): the old bubble goes, and the new view's shows
    /// after `TIP_DELAY` if it has a tooltip.
    fn tip_hover(&mut self, node: i32) {
        self.hide_tip();
        if node != 0 && self.window_focused && find(&mut self.root, node).is_some_and(|m| m.tip.is_some()) {
            self.tip_due = Some(now_ms() + TIP_DELAY);
        }
    }
    fn hide_tip(&mut self) {
        self.tip_due = None;
        for m in std::mem::take(&mut self.tip_view) {
            m.destroy();
        }
    }
    /// The bubble of the hovered view, in the tooltip layer: under it, or above if the screen's bottom
    /// is near, as wide as its text (at most 440) and kept inside the screen.
    fn show_tip(&mut self) {
        let Some(layer) = self.facts["layers"]["tooltip"].as_i64().filter(|n| scene::exists(*n as i32)) else { return };
        let node = self.hover;
        let Some(text) = find(&mut self.root, node).and_then(|m| m.tip.clone()) else { return };
        let Some((x, y, _, h)) = scene::frame(node) else { return };
        let screen = scene::frame(layer as i32).filter(|f| f.2 > 0. && f.3 > 0.).map_or((1920., 1080.), |f| (f64::from(f.2), f64::from(f.3)));
        let measured = super::menu::measure(&text, 12., 400.);
        let width = 440f64.min(screen.0 - 16.).min(measured.ceil() + 20.).max(100.);
        let lines = (measured / (width - 20.)).ceil().max(1.);
        let height = (lines * 17. + 16.).min(screen.1 - 16.);
        let (x, y, h) = (f64::from(x), f64::from(y), f64::from(h));
        let left = x.min(screen.0 - width - 8.).max(8.);
        let top = if y + h + height + 8. < screen.1 { y + h + 6. } else { (y - height - 6.).max(8.) };
        let spec = json!({"kind":"view","layer":"tooltip","key":"tip","style":{"position":"absolute","left":left,"top":top,"width":width,
            "paddingHorizontal":10,"paddingVertical":8,"borderRadius":7,"pointerEvents":"none",
            "backgroundColor":self.theme("popupBg", if self.dark() { "#34343a" } else { "#fafafa" }),
            "border":format!("1px solid {}", self.theme("separator", "rgba(128,128,128,0.3)").as_str().unwrap_or("")),
            "boxShadow":"0 4px 14px rgba(0,0,0,0.25)"},
            "children":[{"kind":"text","label":text,"style":{"fontSize":12,"lineHeight":1.4,"color":self.theme("text", if self.dark() { "#ffffff" } else { "#1d1d1f" })}}]});
        let old = std::mem::take(&mut self.tip_view);
        self.tip_view = self.reconcile(self.mount, old, std::slice::from_ref(&spec));
    }
    /// A press at `target`: the layered views it is outside of (a menu, a popover) get their
    /// `onDismiss`. True if one was sent (the caller syncs).
    fn dismiss(&mut self, target: i32) -> bool {
        let mut actions = Vec::new();
        // A layered view inside another (a menu's submenu) is part of it.
        fn reaches(m: &Mounted, target: i32) -> bool {
            scene::within(target, m.node) || m.children.iter().any(|c| reaches(c, target))
        }
        for p in self.portals.clone() {
            let Some(m) = find(&mut self.root, p) else { continue };
            if reaches(m, target) {
                continue;
            }
            let action = m.spec["events"]["onDismiss"].clone();
            if action.is_array() {
                actions.push(action);
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
                }
                layered(&m.children, out);
            }
        }
        self.portals.clear();
        layered(&self.root, &mut self.portals);
        fn walk(list: &[Mounted], t: &mut HashMap<i32, Target>, b: &mut HashMap<i32, i32>, ev: &mut HashMap<i32, Value>) {
            for m in list {
                if m.kind == Kind::View && m.spec["events"].is_object() {
                    ev.insert(m.node, m.spec["events"].clone());
                }
                if m.tip.is_some() {
                    b.entry(m.node).or_insert(m.node);
                }
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
                walk(&m.children, t, b, ev);
            }
        }
        // Keep the buckets across reconciliations: Calculator has the same
        // keys on every hover/press, and Monitor repeatedly rebuilds its tabs.
        self.targets.clear();
        self.buttons.clear();
        self.events.clear();
        walk(&self.root, &mut self.targets, &mut self.buttons, &mut self.events);
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
                let lit = host.hover == m.node;
                let shown = if lit { json!([m.base, m.spec["hover"]]) } else { m.base.clone() };
                m.style(0, shown);
                lit_children(&mut m.children, lit);
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
        self.tip_hover(node);
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
        if node != 0 {
            // the caret at the end, or the selection the field asked for (select: [from, to], selectAll)
            if let Some(m) = find(&mut self.root, node) {
                let n = m.label.chars().count();
                let (from, to) = match m.spec["select"].as_array() {
                    Some(r) if r.len() == 2 => (r[0].as_u64().unwrap_or(0) as usize, r[1].as_u64().unwrap_or(0) as usize),
                    _ if m.spec["selectAll"] == true => (0, n),
                    _ => (n, n),
                };
                self.anchor = from.min(n);
                self.caret = to.min(n);
                self.caret_value = m.label.clone();
            }
        }
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
        if let Some(info) = &self.pointer_info {
            for (k, v) in info {
                input.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
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
    /// The nearest ancestor of `node` (itself first) with an `events` entry `name`, and its action.
    fn ancestor_event(&self, mut node: i32, name: &str) -> Option<(i32, Value)> {
        for _ in 0..4096 {
            if node <= 0 || node == self.mount {
                return None;
            }
            if let Some(action) = self.events.get(&node).map(|e| &e[name]).filter(|a| a.is_array()) {
                return Some((node, action.clone()));
            }
            node = scene::parent(node);
        }
        None
    }
    /// What the app is told about a pointer event: where (in the view), the button and the modifiers.
    fn pointer_data(&self, x: f32, y: f32, button: i32) -> Map<String, Value> {
        let (fx, fy) = scene::frame(self.mount).map_or((0., 0.), |f| (f.0, f.1));
        let m = super::modifiers();
        Map::from_iter([
            ("x".into(), json!(x - fx)),
            ("y".into(), json!(y - fy)),
            ("button".into(), json!(button)),
            ("shift".into(), json!(m & 1 != 0)),
            ("ctrl".into(), json!(m & 2 != 0)),
            ("alt".into(), json!(m & 4 != 0)),
            ("meta".into(), json!(m & 16 != 0)),
        ])
    }
    /// Some(mount) when the event is this view's: JS then sees it on the mount node.
    pub fn pointer(&mut self, kind: &str, target: i32, x: f32, y: f32, button: i32) -> Option<i32> {
        self.pointer_info = Some(self.pointer_data(x, y, button));
        if self.capture && kind == "move" {
            self.dispatch(&json!(["pointerMove"]), Map::new());
        }
        let out = self.pointer_event(kind, target, x, y, button);
        if self.capture && kind == "up" && button == 0 {
            self.dispatch(&json!(["dragEnd"]), Map::new());
        }
        self.pointer_info = None;
        out
    }
    fn pointer_event(&mut self, kind: &str, target: i32, x: f32, y: f32, button: i32) -> Option<i32> {
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
                if inside {
                    if let Some((_, action)) = self.ancestor_event(target, "onMouseMove") {
                        self.dispatch(&action, Map::new());
                    }
                }
            }
            "down" => {
                self.hide_tip();
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
                    if button == 2 && inside {
                        if let Some((_, action)) = self.ancestor_event(target, "onContextMenu") {
                            self.dispatch(&action, Map::new());
                        }
                    }
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
                // a second click on the same view soon after: onDoubleClick (after its onClick, as in JS)
                if inside {
                    if let Some((node, action)) = self.ancestor_event(target, "onDoubleClick") {
                        let now = now_ms();
                        if self.last_click.0 == node && now - self.last_click.1 < 450. {
                            self.last_click = (0, 0.);
                            self.send(&action, Map::new());
                            changed = true;
                        } else {
                            self.last_click = (node, now);
                        }
                    }
                }
                if changed {
                    self.sync();
                }
            }
            "wheel" if inside => {
                self.hide_tip();
                let mods = super::modifiers();
                let wheel = self.ancestor_event(target, "onWheel");
                let scroll = self.target(target, |t| matches!(t, Target::Scroll));
                match (wheel, scroll) {
                    // Ctrl or Cmd with the wheel is the app's (zoom), or any wheel where nothing scrolls
                    (Some((_, action)), scroll) if mods & (2 | 16) != 0 || scroll.is_none() => {
                        let extra = Map::from_iter([
                            ("deltaY".into(), json!(-(button as f64) * WHEEL_STEP)),
                            ("notches".into(), json!(button)),
                        ]);
                        self.send(&action, extra);
                        self.sync();
                    }
                    (_, Some((node, _))) => self.scroll_by(node, -(button as f64) * WHEEL_STEP),
                    _ => {}
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
    /// A field's new text and where its caret and selection are after it.
    fn edit_to(&mut self, text: String, caret: usize, anchor: usize) {
        self.caret = caret;
        self.anchor = anchor;
        self.caret_value = text.clone();
        self.edit(text);
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
        self.hide_tip();
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
        let altgr = flags & 8 != 0;
        let plain_alt = alt && !altgr;
        let mut field = crate::text_edit::Field::new(&value, self.caret, self.anchor);
        // Alt is for the shell's shortcuts (and word moves), not a typed character; AltGr types.
        let out = field.key(
            crate::text_edit::Key {
                key,
                chr: if plain_alt { "" } else { chr },
                command: ctrl || meta,
                alt: plain_alt,
                shift: flags & 1 != 0,
                base: key,
            },
            false,
        );
        if !out.handled {
            return false;
        }
        if out.paste {
            self.effects.push_back(json!({"kind":"paste","host":true}));
            return true;
        }
        if let Some(text) = out.copy {
            self.effects.push_back(json!({"kind":"copy","text":text,"host":true}));
        }
        if out.cancel {
            let cancel = find(&mut self.root, self.field).map(|m| m.spec["cancel"].clone()).filter(Value::is_array);
            self.blur();
            if let Some(action) = cancel {
                self.dispatch(&action, Map::new());
            }
        } else if out.submit {
            let submit = find(&mut self.root, self.field).map(|m| m.spec["submit"].clone()).filter(Value::is_array);
            if let Some(action) = submit {
                self.blur();
                self.dispatch(&action, Map::new());
            }
        } else if out.changed {
            let text = field.value();
            if text.chars().count() <= 2048 {
                self.edit_to(text, field.caret, field.anchor);
            }
        } else {
            // only the caret or the selection moved
            self.caret = field.caret;
            self.anchor = field.anchor;
            self.caret_on = true;
            self.next_blink = Some(now_ms() + CARET_BLINK);
            self.restyle(self.field);
        }
        true
    }
    pub fn paste(&mut self, text: &str) {
        if let Some(value) = self.field_value() {
            let mut field = crate::text_edit::Field::new(&value, self.caret, self.anchor);
            if field.insert(text) && field.value().chars().count() <= 2048 {
                self.edit_to(field.value(), field.caret, field.anchor);
            }
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
        if self.tip_due.is_some_and(|t| now >= t) {
            self.tip_due = None;
            self.show_tip();
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
            self.tip_due,
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

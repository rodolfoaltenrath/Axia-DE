use super::*;

pub(super) const START_DEFAULT: [&str; 16] = [
    "files",
    "terminal",
    "editor",
    "calculator",
    "notes",
    "photos",
    "player",
    "mail",
    "calendar",
    "contacts",
    "store",
    "settings",
    "monitor",
    "clock",
    "help",
    "about",
];

pub(super) fn dock_id(a: &Value) -> Option<String> {
    let kind = string(a, "kind");
    let id = if kind == "linux" {
        string(a, "desktopId")
    } else {
        string(a, "id")
    };
    let valid = if kind == "linux" {
        !id.is_empty()
            && id.len() <= 62
            && !id.starts_with(['.', '-'])
            && !id.starts_with("lpk-")
            && !id.contains("..")
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    } else {
        id.len() >= if kind == "package" { 2 } else { 1 }
            && id.len() <= 40
            && id.as_bytes()[0].is_ascii_lowercase()
            && id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    };
    valid.then(|| {
        format!(
            "{}:{id}",
            if kind == "linux" {
                "desktop"
            } else if kind == "package" {
                "lpk"
            } else {
                "app"
            }
        )
    })
}

pub(super) fn placement(a: &Value) -> Value {
    let d = &a["display"];
    let (x, y, w, h) = (
        number(d, "x", 0., -1e6, 1e6),
        number(d, "y", 0., -1e6, 1e6),
        number(d, "w", 1000., 1., 16384.),
        number(d, "h", 700., 1., 16384.),
    );
    let pos = string(a, "position");
    let vertical = pos == "left" || pos == "right";
    let width = 640_f64.min((w - 24.).max(1.));
    let height = 700_f64.min((h - if vertical { 0. } else { 48. } - 24.).max(1.));
    let left = if vertical {
        if pos == "left" {
            x + 56. + 12.
        } else {
            x + w - 56. - 12. - width
        }
    } else if string(a, "alignment") == "start" {
        if truthy(&a["rtl"]) {
            x + w - 12. - width
        } else {
            x + 12.
        }
    } else {
        x + ((w - width) / 2.).round()
    };
    let top = if pos == "top" {
        y + 48. + 12.
    } else if pos == "bottom" {
        y + h - 48. - 12. - height
    } else {
        y + 12.
    };
    json!({"left":left,"top":top,"width":width,"height":height})
}

#[derive(Default)]
pub(super) struct Menu {
    pub id: u64,
    pub classic: bool,
    pub context: Value,
    pub query: String,
    pub category: String,
    pub all: bool,
    pub selected: usize,
    pub popup: Option<Value>,
    pub power: Option<Value>,
    pub hover: Option<String>,
    pub results: Vec<Value>,
    pub scroll_generation: u64,
    pub offsets: HashMap<String, (f64, f64)>,
    pub effects: Vec<Value>,
    pub handled: bool,
    pub plan: Value,
}

impl Menu {
    pub fn pins(&self, model: &AxiaLauncher) -> Vec<String> {
        let defaults = json!(START_DEFAULT);
        let raw = self.context["settings"]["startPinned"]
            .as_array()
            .unwrap_or_else(|| defaults.as_array().unwrap());
        raw.iter()
            .filter_map(|id| id.as_str())
            .filter(|id| {
                model
                    .entries
                    .iter()
                    .any(|a| string(&a.metadata, "id") == *id)
            })
            .map(str::to_owned)
            .collect()
    }
    pub fn recent(&self, model: &AxiaLauncher) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(raw) = self.context["settings"]["dockRecentApps"].as_array() {
            for id in raw.iter().filter_map(Value::as_str) {
                if let Some(a) = model.entries.iter().find(|a| {
                    string(&a.metadata, "dockId") == id || string(&a.metadata, "id") == id
                }) {
                    let key = string(&a.metadata, "id").to_owned();
                    if !out.contains(&key) {
                        out.push(key);
                    }
                    if out.len() == 6 {
                        break;
                    }
                }
            }
        }
        out
    }
    fn flat(&self) -> Vec<&Value> {
        self.results
            .iter()
            .filter_map(|s| s["items"].as_array())
            .flatten()
            .collect()
    }
    fn search(&mut self) {
        self.effects
            .push(json!({"type":"search","query":self.query}));
    }
    fn open(&mut self, model: &AxiaLauncher, id: &str) {
        if model
            .entries
            .iter()
            .any(|a| string(&a.metadata, "id") == id)
        {
            self.effects
                .extend([json!({"type":"close"}), json!({"type":"open","id":id})]);
        }
    }
    fn run(&mut self, key: &str) {
        if self.flat().iter().any(|a| string(a, "key") == key) {
            self.effects.extend([
                json!({"type":"close"}),
                json!({"type":"runSearch","key":key}),
            ]);
        }
    }
    fn reset_scroll(&mut self) {
        self.scroll_generation = self.scroll_generation.wrapping_add(1);
        self.offsets.clear();
    }
    pub fn event(&mut self, model: &mut AxiaLauncher, a: &Value) {
        let action = &a["action"];
        let name = action[0].as_str().unwrap_or("");
        let target = action[1].as_str().unwrap_or("");
        self.handled = false;
        match name {
            "query" => {
                if target.len() > 16384 {
                    return;
                }
                self.query = target.to_owned();
                self.selected = 0;
                self.results.clear();
                self.reset_scroll();
                if self.classic {
                    self.search();
                }
            }
            "category" => {
                if target.is_empty() || CATEGORIES.contains(&target) {
                    self.category = target.to_owned();
                    self.reset_scroll();
                }
            }
            "all" => {
                self.all = true;
                self.reset_scroll();
            }
            "home" => {
                self.all = false;
                self.reset_scroll();
            }
            "cancel" => {
                if !self.classic || target.is_empty() {
                    self.effects.push(json!({"type":"close"}));
                }
            }
            "outside" => {
                if self.classic && !truthy(&a["inside"]) && !truthy(&a["onBar"]) {
                    self.effects.push(json!({"type":"close"}));
                }
            }
            "close" => {
                if !truthy(&a["insidePopup"]) {
                    self.effects.push(json!({"type":"close"}));
                }
            }
            "dragStarted" => self.effects.push(json!({"type":"close"})),
            "open" => {
                if !truthy(&a["dragJustEnded"]) {
                    self.open(model, target);
                }
            }
            "submit" => {
                if self.classic {
                    if !unicode::query(&self.query).is_empty() {
                        let key = self
                            .flat()
                            .get(self.selected)
                            .map(|v| string(v, "key").to_owned());
                        if let Some(key) = key {
                            self.run(&key);
                        }
                    }
                } else if let Ok(v) =
                    model.view(&json!({"query":self.query,"category":self.category,"height":1}))
                {
                    if let Some(i) = v["first"].as_u64() {
                        let id = string(&model.entries[i as usize].metadata, "id").to_owned();
                        self.open(model, &id);
                    }
                }
            }
            "key" => {
                if self.classic && !unicode::query(&self.query).is_empty() {
                    let len = self.flat().len();
                    match target {
                        "ArrowDown" => {
                            self.selected = (self.selected + 1).min(len.saturating_sub(1));
                            self.handled = true;
                        }
                        "ArrowUp" => {
                            self.selected = self.selected.saturating_sub(1);
                            self.handled = true;
                        }
                        "Enter" => {
                            let key = self
                                .flat()
                                .get(self.selected)
                                .map(|v| string(v, "key").to_owned());
                            if let Some(key) = key {
                                self.run(&key);
                            }
                            self.handled = true;
                        }
                        _ => {}
                    }
                }
            }
            "select" => {
                if let Some(i) = action[1].as_u64() {
                    self.selected = (i as usize).min(self.flat().len().saturating_sub(1));
                }
            }
            "hover" => {
                if action[2] == true {
                    self.hover = Some(target.to_owned());
                } else if self.hover.as_deref() == Some(target) {
                    self.hover = None;
                }
            }
            "context" => {
                if model
                    .entries
                    .iter()
                    .any(|a| string(&a.metadata, "id") == target)
                {
                    self.popup = Some(
                        json!({"id":target,"x":number(a,"x",0.,-1e6,1e6),"y":number(a,"y",0.,-1e6,1e6),"dock":a["dock"]}),
                    );
                }
            }
            "closeContext" => {
                self.popup = None;
            }
            "closePower" => {
                self.power = None;
            }
            "dock" => {
                let d = &a["dock"];
                if model
                    .entries
                    .iter()
                    .any(|app| string(&app.metadata, "id") == target)
                    && truthy(&d["id"])
                    && (!truthy(&d["pinned"]) || truthy(&d["removable"]))
                {
                    self.popup = None;
                    self.effects
                        .push(json!({"type":"dock","id":target,"pin":!truthy(&d["pinned"])}));
                }
            }
            "pinStart" => {
                if model
                    .entries
                    .iter()
                    .any(|app| string(&app.metadata, "id") == target)
                {
                    self.popup = None;
                    let id = target;
                    let on = !self.pins(model).iter().any(|x| x == id);
                    let mut pins = self.context["settings"]["startPinned"]
                        .as_array()
                        .cloned()
                        .unwrap_or_else(|| START_DEFAULT.iter().map(|s| json!(s)).collect());
                    pins.retain(|v| v.as_str() != Some(id));
                    if on {
                        pins.push(json!(id));
                    }
                    self.effects
                        .push(json!({"type":"settings","patch":{"startPinned":pins}}));
                }
            }
            "drag" => {
                if a["button"].as_i64() == Some(0)
                    && model.entries.iter().any(|v| {
                        string(&v.metadata, "id") == target && v.metadata["dockId"].is_string()
                    })
                {
                    self.effects
                        .push(json!({"type":"drag","id":target,"x":a["x"],"y":a["y"]}));
                }
            }
            "powerMenu" => {
                self.power = Some(
                    json!({"x":number(a,"x",0.,-1e6,1e6)-200.,"y":number(a,"y",0.,-1e6,1e6)-12.}),
                );
            }
            "session" => {
                let allowed = match target {
                    "quit" => true,
                    "lock" | "switchUser" => truthy(&self.context["session"]["managed"]),
                    "restart" | "shutdown" => truthy(&self.context["session"]["canPower"]),
                    _ => false,
                };
                if allowed {
                    self.effects.extend([
                        json!({"type":"close"}),
                        json!({"type":"session","action":target}),
                    ]);
                }
            }
            "user" => {
                self.effects
                    .extend([json!({"type":"close"}), json!({"type":"userSettings"})]);
            }
            "run" => self.run(target),
            "searchRefresh" => {
                if self.classic && !unicode::query(&self.query).is_empty() {
                    self.search();
                }
            }
            "results" => {
                if string(a, "query") == self.query {
                    if let Some(sections) = a["sections"].as_array().filter(|s| s.len() <= 16) {
                        self.results = sections
                            .iter()
                            .filter(|s| s["items"].as_array().is_some_and(|i| i.len() <= 64))
                            .cloned()
                            .collect();
                        self.selected = self.selected.min(self.flat().len().saturating_sub(1));
                    }
                }
            }
            _ => {}
        }
    }
}

impl AxiaLauncher {
    pub(super) fn menu_request(&mut self, args: &Value) -> Result<Value, &'static str> {
        let op = string(args, "op");
        if op == "open" {
            if self.menus.len() >= 16 {
                return Err("too many launcher menus");
            }
            let classic = match string(args, "mode") {
                "flow" => false,
                "classic" => true,
                _ => return Err("invalid menu mode"),
            };
            self.sequence = self.sequence.wrapping_add(1);
            let mut menu = Menu {
                id: self.sequence,
                classic,
                context: args["context"].clone(),
                ..Menu::default()
            };
            if classic {
                menu.effects.push(json!({"type":"prepareSearch"}));
            }
            let reply = menu.snapshot(self);
            self.menus.insert(menu.id, menu);
            return Ok(reply);
        }
        let id = args["id"].as_u64().ok_or("invalid menu handle")?;
        let mut menu = self.menus.remove(&id).ok_or("unknown menu handle")?;
        if op == "close" {
            return Ok(Value::Null);
        }
        let reply = match op {
            "context" => {
                menu.context = args["context"].clone();
                menu.snapshot(self)
            }
            "event" => {
                menu.event(self, args);
                menu.snapshot(self)
            }
            "scroll" => {
                let key = string(args, "key");
                if !["grid", "all", "pins", "search"].contains(&key) {
                    self.menus.insert(id, menu);
                    return Err("invalid scroll region");
                }
                menu.offsets.insert(
                    key.to_owned(),
                    (
                        number(args, "offset", 0., 0., 1e9),
                        number(args, "height", 600., 1., 16384.),
                    ),
                );
                let tree = ui::scroll(&mut menu, self, key);
                json!({"tree":tree,"total":menu.plan["total"],"first":menu.first_id(self),"offset":menu.plan["offset"]})
            }
            _ => {
                self.menus.insert(id, menu);
                return Err("invalid menu operation");
            }
        };
        self.menus.insert(id, menu);
        Ok(reply)
    }
}

impl Menu {
    fn first_id(&self, model: &AxiaLauncher) -> Value {
        self.plan["first"]
            .as_u64()
            .and_then(|i| model.entries.get(i as usize))
            .map(|a| a.metadata["id"].clone())
            .unwrap_or(Value::Null)
    }
    fn snapshot(&mut self, model: &mut AxiaLauncher) -> Value {
        let tree = ui::render(self, model);
        json!({"id":self.id,"tree":tree,"query":self.query,"handled":self.handled,"generation":model.generation,
            "effects":std::mem::take(&mut self.effects),"total":self.plan["total"],"first":self.first_id(model),"offset":self.plan["offset"]})
    }
}

use super::*;

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

#[derive(Default)]
pub(super) struct Menu {
    pub id: u64,
    pub context: Value,
    pub query: String,
    pub category: String,
    pub popup: Option<Value>,
    pub scroll_generation: u64,
    pub offsets: HashMap<String, (f64, f64)>,
    pub effects: Vec<Value>,
    pub handled: bool,
    pub plan: Value,
}

impl Menu {
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
                self.reset_scroll();
            }
            "category" => {
                if target.is_empty() || CATEGORIES.contains(&target) {
                    self.category = target.to_owned();
                    self.reset_scroll();
                }
            }
            "cancel" => self.effects.push(json!({"type":"close"})),
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
                if let Ok(v) =
                    model.view(&json!({"query":self.query,"category":self.category,"height":1}))
                {
                    if let Some(i) = v["first"].as_u64() {
                        let id = string(&model.entries[i as usize].metadata, "id").to_owned();
                        self.open(model, &id);
                    }
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
            if string(args, "mode") != "flow" {
                return Err("invalid menu mode");
            }
            self.sequence = self.sequence.wrapping_add(1);
            let mut menu = Menu {
                id: self.sequence,
                context: args["context"].clone(),
                ..Menu::default()
            };
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
                if key != "grid" {
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

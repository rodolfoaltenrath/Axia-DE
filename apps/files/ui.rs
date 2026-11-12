//! Native widget descriptions. Layout, menus and dialogs are authored in Rust;
//! the SDK adapter only binds these nodes to the existing Axia renderer.
use super::{
    model::{archive, kind, Dialog, Location, Mode, Session, Shared},
    operations::Entry,
    thumbnails,
};
use serde_json::{json, Value};
use std::collections::HashSet;

fn node(kind: &str, style: Value, children: Vec<Value>) -> Value {
    json!({"type":kind,"style":style,"children":children})
}
fn view(style: Value, children: Vec<Value>) -> Value {
    node("view", style, children)
}
fn label(text: Value, style: Value) -> Value {
    json!({"type":"text","text":text,"style":style})
}
fn file_label(s: &Session, text: Value, full: Value, style: Value) -> Value {
    json!({"type":"filename","text":text,"tooltip":full,"tooltipLayer":if s.mode==Mode::Browser{"tooltip"}else{"system"},"style":style})
}
pub fn tr(key: &str) -> Value {
    json!({"key":key})
}
fn button(text: Value, action: Value, disabled: bool) -> Value {
    json!({"type":"button","label":text,"action":action,"disabled":disabled})
}
fn input(value: &str, action: &str, placeholder: Value) -> Value {
    let mut field = json!({"type":"input","value":value,"action":[action],"placeholder":placeholder,"style":{"flex":1,"height":30}});
    match action {
        "dialogText" => {
            field["submit"] = json!(["submit"]);
            field["cancel"] = json!(["cancelDialog"]);
            field["autoFocus"] = json!(true);
            field["selectAll"] = json!(true);
        }
        "query" => {
            field["submit"] = json!(["search"]);
        }
        "name" => {
            field["autoFocus"] = json!(true);
            field["selectAll"] = json!(true);
            field["submit"] = json!(["choose"]);
            field["cancel"] = json!(["cancel"]);
        }
        "comment" => {
            field["cancel"] = json!(["cancelDialog"]);
        }
        _ => (),
    }
    field
}
fn color(s: &Session, key: &str, default: &str) -> String {
    s.theme[key].as_str().unwrap_or(default).into()
}
fn txt(s: &Session) -> String {
    color(s, "text", "#ececf2")
}
fn dim(s: &Session) -> String {
    color(s, "textDim", "#a4a4af")
}
fn row() -> Value {
    json!({"flexDirection":"row","alignItems":"center","gap":8})
}
fn action(name: &str) -> Value {
    json!([name])
}
pub fn bytes(n: u64) -> String {
    if n < 1024 {
        return format!("{n} B");
    }
    let mut number = n as f64;
    let mut unit = "B";
    for u in ["KB", "MB", "GB", "TB"] {
        number /= 1024.;
        unit = u;
        if number < 1024. {
            break;
        }
    }
    format!("{number:.1} {unit}")
}

// A named icon in the widget tree; the host asks once for its geometry.
fn icon_ref(name: &str, size: f64) -> Value {
    json!({"type":"icon","name":name,"size":size})
}
// Compositor primitives, independent of emoji fonts and platform icon themes.
pub fn icon(name: &str, size: f64) -> Value {
    if name == "files" {
        return view(
            json!({"width":size,"height":size,"borderRadius":size*0.225,"background":"linear-gradient(180deg, #74c7ff, #1f6fe8)","boxShadow":format!("0 {}px {}px rgba(0,0,0,0.35)",size*0.04,size*0.12),"overflow":"hidden"}),
            vec![
                view(
                    json!({"position":"absolute","left":size*0.2,"top":size*0.27,"width":size*0.27,"height":size*0.12,"borderRadius":size*0.03,"backgroundColor":"rgba(255,255,255,0.75)"}),
                    vec![],
                ),
                view(
                    json!({"position":"absolute","left":size*0.2,"top":size*0.33,"width":size*0.6,"height":size*0.42,"borderRadius":size*0.06,"backgroundColor":"rgba(255,255,255,0.95)"}),
                    vec![],
                ),
            ],
        );
    }
    let folder = name == "folder";
    let base = if folder {
        "#4aaeff"
    } else if name == "trash" {
        "#939aaa"
    } else {
        "#d0d8e8"
    };
    let mut children = Vec::new();
    if folder {
        children.push(view(json!({"position":"absolute","left":size*0.1,"top":size*0.13,"width":size*0.38,"height":size*0.25,"borderRadius":size*0.06,"backgroundColor":"#85ceff"}),vec![]));
        children.push(view(json!({"position":"absolute","left":size*0.08,"top":size*0.29,"width":size*0.84,"height":size*0.59,"borderRadius":size*0.07,"backgroundColor":base}),vec![]));
    } else if name == "drive" {
        children.push(view(json!({"position":"absolute","left":size*0.08,"top":size*0.2,"width":size*0.84,"height":size*0.63,"borderRadius":size*0.12,"backgroundColor":"#a2aab9"}),vec![]));
        children.push(view(json!({"position":"absolute","left":size*0.18,"top":size*0.62,"width":size*0.64,"height":size*0.06,"backgroundColor":"#596677"}),vec![]));
    } else if name == "trash" {
        children.push(view(json!({"position":"absolute","left":size*0.25,"top":size*0.27,"width":size*0.5,"height":size*0.6,"borderRadius":size*0.06,"border":format!("{}px solid {base}",(size*0.065).max(1.))}),vec![]));
        children.push(view(json!({"position":"absolute","left":size*0.17,"top":size*0.18,"width":size*0.66,"height":(size*0.08).max(1.),"backgroundColor":base}),vec![]));
    } else {
        children.push(view(json!({"position":"absolute","left":size*0.22,"top":size*0.08,"width":size*0.57,"height":size*0.82,"borderRadius":size*0.06,"backgroundColor":base}),vec![]));
        for y in [0.42, 0.55, 0.68] {
            children.push(view(json!({"position":"absolute","left":size*0.33,"top":size*y,"width":size*0.34,"height":(size*0.04).max(1.),"backgroundColor":"#75859c"}),vec![]));
        }
    }
    view(json!({"width":size,"height":size,"flexShrink":0}), children)
}
// Buffered writes can reach the byte total before fsync and publication finish.
// Reserve 100% for a successfully completed operation, including zero-byte jobs.
fn transfer_percent(job: &Value) -> u64 {
    if job["done"] == true && job["failed"] != true && job["cancel"] != true {
        return 100;
    }
    let total = job["total"].as_u64().unwrap_or(0);
    let bytes = job["bytes"].as_u64().unwrap_or(0);
    if total > 0 {
        (bytes as u128 * 100 / total as u128).min(99) as u64
    } else {
        0
    }
}

#[cfg(test)]
mod transfer_tests {
    use super::*;
    #[test]
    fn only_successfully_published_transfers_reach_one_hundred_percent() {
        assert_eq!(transfer_percent(&json!({"bytes":50,"total":200})), 25);
        assert_eq!(
            transfer_percent(&json!({"bytes":200,"total":200,"phase":"syncing"})),
            99
        );
        assert_eq!(transfer_percent(&json!({"bytes":250,"total":200})), 99);
        assert_eq!(
            transfer_percent(&json!({"bytes":200,"total":200,"done":true,"failed":true})),
            99
        );
        assert_eq!(
            transfer_percent(&json!({"bytes":200,"total":200,"done":true,"cancel":true})),
            99
        );
        assert_eq!(transfer_percent(&json!({"total":0})), 0);
        assert_eq!(transfer_percent(&json!({"total":0,"done":true})), 100);
    }
}
fn entry_icon(s: &Session, e: &Entry, size: f64, thumb: bool) -> Value {
    if thumb && !e.dir && kind(&e.name) == "pdf" {
        if let Some(image) = s.pdf_thumbnails.get(e) {
            return json!({"type":"documentImage","image":image,"style":{"width":size,"height":size,"objectFit":"contain","borderRadius":3}});
        }
    }
    if thumb && !e.dir && kind(&e.name) == "image" {
        if let Some(path) = thumbnails::get(e) {
            return json!({"type":"image","src":path,"style":{"width":size,"height":size,"objectFit":"contain","borderRadius":4}});
        }
    }
    icon_ref(if e.dir { "folder" } else { "file" }, size)
}
fn side_item(s: &Session, text: Value, command: Value, selected: bool, kind: &str) -> Value {
    let traditional = s.theme["nativeStyle"] == "adwaita";
    let selected_bg = if traditional {
        color(s, "accent", "#3584e4")
    } else if s.theme["mode"] == "light" {
        "rgba(0,0,0,0.08)".into()
    } else {
        "rgba(255,255,255,0.12)".into()
    };
    let mut n = view(
        json!({"height":28,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":8,"paddingHorizontal":8,"borderRadius":if traditional{3}else{6},"backgroundColor":if selected{selected_bg}else{"transparent".into()}}),
        vec![
            icon_ref(kind, 19.),
            label(
                text.clone(),
                json!({"fontSize":13,"color":if selected&&traditional{"#ffffff".into()}else{txt(s)},"flex":1,"minWidth":0,"whiteSpace":"nowrap","textOverflow":"ellipsis","numberOfLines":1,"overflow":"hidden"}),
            ),
        ],
    );
    n["type"] = json!("fileTile");
    n["tooltip"] = text;
    n["tooltipLayer"] = json!(if s.mode == Mode::Browser {
        "tooltip"
    } else {
        "system"
    });
    n["events"] = json!({"onClick":command});
    if s.mode == Mode::Browser && command[0] == "navigate" {
        n["events"]["onMouseUp"] = json!(["drop", command[1]]);
    } else if s.mode == Mode::Browser && command[0] == "place" && command[1] == "trash" {
        n["events"]["onMouseUp"] = json!(["drop", "trash"]);
    }
    n
}

impl Session {
    pub fn title_value(&self) -> Value {
        self.location_title(&self.tab().location)
    }
    fn location_title(&self, location: &Location) -> Value {
        match location {
            Location::Directory(p) => {
                if *p == self.policy.home {
                    tr("files.place.home")
                } else if p == std::path::Path::new("/") {
                    tr("files.place.computer")
                } else if let Some(place) = self.places.iter().find(|place| place.path == *p) {
                    tr(&format!("files.place.{}", place.label))
                } else {
                    json!(p.file_name().unwrap_or(p.as_os_str()).to_string_lossy())
                }
            }
            Location::Trash => tr("files.place.trash"),
            Location::Recent => tr("files.place.recent"),
            Location::Apps => tr("files.place.apps"),
            Location::Network => tr("files.place.network"),
            Location::Tag(c) => json!(c),
            Location::Archive { file, inner } => json!(if inner.is_empty() {
                file.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            } else {
                inner.clone()
            }),
        }
    }
    fn sidebar(&self, shared: &Shared) -> Value {
        let mut items = vec![label(
            tr("files.rust.favorites"),
            json!({"color":dim(self),"fontSize":11,"padding":8}),
        )];
        items.push(side_item(
            self,
            tr("files.place.home"),
            json!(["place", "home"]),
            self.tab().location == Location::Directory(self.policy.home.clone()),
            "folder",
        ));
        let hidden = shared.preferences["hiddenSide"].as_array();
        for place in &self.places {
            if hidden.is_some_and(|v| v.iter().any(|i| i.as_str() == Some(&place.label))) {
                continue;
            }
            items.push(side_item(
                self,
                tr(&format!("files.place.{}", place.label)),
                json!(["navigate", place.path]),
                self.tab().location == Location::Directory(place.path.clone()),
                "folder",
            ));
        }
        for favorite in shared.preferences["favoriteFolders"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(path) = favorite["path"].as_str() {
                items.push(side_item(
                    self,
                    json!(favorite["name"].as_str().unwrap_or(path)),
                    json!(["navigate", path]),
                    self.tab().location == Location::Directory(path.into()),
                    "folder",
                ));
            }
        }
        if self.mode == Mode::Browser {
            items.push(label(
                tr("files.rust.locations"),
                json!({"color":dim(self),"fontSize":11,"padding":8,"marginTop":8}),
            ));
            for (key, location, ico) in [
                ("recent", Location::Recent, "file"),
                ("computer", Location::Directory("/".into()), "file"),
                ("apps", Location::Apps, "file"),
                ("network", Location::Network, "folder"),
                ("trash", Location::Trash, "trash"),
            ] {
                items.push(side_item(
                    self,
                    tr(&format!("files.place.{key}")),
                    json!(["place", key]),
                    self.tab().location == location,
                    ico,
                ));
            }
        }
        if !shared.volumes.is_empty() {
            items.push(label(
                tr("files.rust.devices"),
                json!({"color":dim(self),"fontSize":11,"padding":8,"marginTop":8}),
            ));
            for volume in &shared.volumes {
                let mut entry = side_item(
                    self,
                    if volume.path.is_none() && !volume.mountable {
                        json!({"key":"files.rust.manageVolume","params":{"name":volume.label}})
                    } else {
                        json!(volume.label)
                    },
                    json!(["volumeOpen", volume.id]),
                    volume.path.as_ref().is_some_and(|p| {
                        self.tab().location.path().is_some_and(|q| q.starts_with(p))
                    }),
                    "drive",
                );
                if self.mode == Mode::Browser {
                    if let Some(path) = &volume.path {
                        entry["events"]["onMouseUp"] = json!(["drop", path]);
                    }
                }
                if volume.path.is_some() {
                    let mut eject = view(
                        json!({"width":22,"height":24,"alignItems":"center","justifyContent":"center","flexShrink":0}),
                        vec![label(json!("⏏"), json!({"color":dim(self),"fontSize":14}))],
                    );
                    eject["events"] = json!({"onClick":["volumeRelease",volume.id]});
                    entry["children"].as_array_mut().unwrap().push(eject);
                }
                if self.volume_busy.as_deref() == Some(&volume.id) {
                    entry["style"]["opacity"] = json!(0.5);
                }
                items.push(entry);
            }
        }
        for (path, _) in &shared.extra_mounts {
            if shared.volumes.iter().any(|v| v.path.as_ref() == Some(path)) {
                continue;
            }
            items.push(side_item(
                self,
                json!(path.file_name().unwrap_or_default().to_string_lossy()),
                json!(["navigate", path]),
                self.tab().location == Location::Directory(path.clone()),
                "folder",
            ));
        }
        if self.mode == Mode::Browser {
            items.push(label(
                tr("files.menu.tags"),
                json!({"color":dim(self),"fontSize":11,"padding":8,"marginTop":8}),
            ));
            for (color, hex) in [
                ("red", "#ff6257"),
                ("orange", "#ff9d33"),
                ("yellow", "#e0c947"),
                ("green", "#53bf70"),
                ("blue", "#479eff"),
                ("purple", "#af7bf4"),
                ("gray", "#9999aa"),
            ] {
                let mut n = view(
                    json!({"height":27,"flexDirection":"row","alignItems":"center","gap":10,"paddingHorizontal":10}),
                    vec![
                        view(
                            json!({"width":9,"height":9,"borderRadius":5,"backgroundColor":hex}),
                            vec![],
                        ),
                        label(
                            shared.preferences["tagNames"][color]
                                .as_str()
                                .map(|v| json!(v))
                                .unwrap_or_else(|| tr(&format!("files.tag.{color}"))),
                            json!({"fontSize":12,"color":txt(self)}),
                        ),
                    ],
                );
                n["events"] = json!({"onClick":["tagPlace",color]});
                items.push(n);
            }
        }
        node(
            "scroll",
            json!({"width":self.side_width,"flex":0,"flexShrink":0,"backgroundColor":color(self,"sidebarBg","#25252b"),"padding":8,"borderRight":format!("1px solid {}",color(self,"separator","#38383e"))}),
            items,
        )
    }
    fn toolbar(&self) -> Value {
        let control = |glyph: &str, key: &str, command: Value, disabled: bool, active: bool| json!({"type":"tool","icon":glyph,"title":tr(key),"action":command,"disabled":disabled,"active":active});
        let available = self.width - self.side_width - 5.;
        let narrow = available < 620.;
        let mut controls = vec![
            control(
                "‹",
                "files.menu.back",
                action("back"),
                self.tab().back.is_empty(),
                false,
            ),
            control(
                "›",
                "files.menu.forward",
                action("forward"),
                self.tab().forward.is_empty(),
                false,
            ),
            file_label(
                self,
                self.title_value(),
                self.title_value(),
                json!({"flex":1,"minWidth":0,"fontSize":15,"fontWeight":600,"color":txt(self),"whiteSpace":"nowrap","textOverflow":"ellipsis","numberOfLines":1,"overflow":"hidden"}),
            ),
        ];
        let modes = [
            ("icons", "▦"),
            ("list", "≡"),
            ("columns", "◫"),
            ("gallery", "▣"),
        ];
        if narrow {
            let at = modes
                .iter()
                .position(|(key, _)| *key == self.tab().view)
                .unwrap_or(0);
            controls.push(control(
                modes[at].1,
                "files.menu.view",
                json!(["view", modes[(at + 1) % 4].0]),
                false,
                false,
            ));
        } else {
            controls.push(node(
                "toolGroup",
                json!({}),
                modes
                    .iter()
                    .map(|(key, glyph)| {
                        control(
                            glyph,
                            &format!("files.view.{key}"),
                            json!(["view", key]),
                            false,
                            self.tab().view == *key,
                        )
                    })
                    .collect(),
            ));
            controls.push(control(
                "↑",
                "files.menu.up",
                action("up"),
                self.tab().location.path().is_none(),
                false,
            ));
            controls.push(control(
                "↻",
                "files.rust.refresh",
                action("refresh"),
                self.visibly_loading(),
                false,
            ));
        }
        controls.push(control(
            "⋯",
            "files.actions",
            json!(["context", null]),
            false,
            false,
        ));
        let mut search = input(&self.tab().query, "query", tr("files.search"));
        search["style"] = json!({"width":if available<330.{90}else if narrow{120}else{180},"height":28,"flexShrink":0});
        controls.push(search);
        node(
            "toolbar",
            if self.theme["nativeStyle"] == "adwaita" {
                json!({"flexShrink":0,"backgroundColor":color(self,"titleBg","#353535")})
            } else {
                json!({"flexShrink":0})
            },
            controls,
        )
    }
    fn search_height(&self) -> f64 {
        if self.tab().query.trim().is_empty() {
            0.
        } else {
            32.
        }
    }
    fn tabs_height(&self) -> f64 {
        if self.mode == Mode::Browser && self.tabs.len() > 1 {
            28.
        } else {
            0.
        }
    }
    pub(super) fn entry_width(&self) -> f64 {
        let past = self.past_columns();
        (self.width - self.side_width - 5. - past as f64 * self.column_width()).max(1.)
    }
    // One geometry for painting, keyboard movement, scrolling and PDF requests.
    // Leave room for the scrollbar, then distribute the full width across cells.
    pub(super) fn entry_grid(&self) -> (usize, f64, f64) {
        let width = (self.entry_width() - 12.).max(1.);
        if self.tab().view == "icons" {
            let columns = (width / (112. * self.zoom)).floor().max(1.) as usize;
            let row = if self.entry_area() < 360. {
                100. * self.zoom
            } else {
                110. * self.zoom
            };
            (columns, width / columns as f64, row)
        } else {
            (1, width, (32. * self.zoom).max(26.))
        }
    }
    pub(super) fn column_row(&self) -> f64 {
        28. * self.zoom
    }
    fn column_width(&self) -> f64 {
        190. * self.zoom
    }
    fn past_columns(&self) -> usize {
        if self.tab().view != "columns" {
            return 0;
        }
        let available = self.width - self.side_width - 5.;
        let capacity = ((available - 160. * self.zoom) / self.column_width())
            .floor()
            .clamp(0., 2.) as usize;
        self.tab().columns.len().min(capacity)
    }
    pub(super) fn entry_height(&self) -> f64 {
        (self.height
            - 44.
            - 26.
            - 27.
            - self.tabs_height()
            - self.search_height()
            - if self.mode == Mode::Browser { 0. } else { 92. }
            - if self.error.is_some() { 44. } else { 0. }
            - self.job_errors.len() as f64 * 32.
            - if self.tab().location == Location::Network {
                if self.discovered.is_empty() {
                    60.
                } else {
                    32.
                }
            } else {
                0.
            })
        .max(1.)
    }
    pub(super) fn entry_area(&self) -> f64 {
        let h = self.entry_height();
        h - self.gallery_height()
    }
    fn gallery_height(&self) -> f64 {
        if self.tab().view == "gallery" {
            (self.entry_height() * 0.65).min((self.entry_height() - 32.).max(0.))
        } else {
            0.
        }
    }
    fn tabs_view(&self) -> Value {
        let capacity =
            (((self.width - self.side_width - 5. - 80.) / 110.).floor() as usize).clamp(1, 32);
        let start = self
            .active
            .saturating_sub(capacity / 2)
            .min(self.tabs.len().saturating_sub(capacity));
        let end = (start + capacity).min(self.tabs.len());
        let mut tabs = Vec::new();
        if start > 0 {
            tabs.push(button(json!("‹"), json!(["tab", start - 1]), false));
        }
        for i in start..end {
            let title = self.location_title(&self.tabs[i].location);
            let mut close = view(
                json!({"width":18,"height":18,"alignItems":"center","justifyContent":"center","flexShrink":0}),
                vec![label(json!("×"), json!({"fontSize":12,"color":dim(self)}))],
            );
            close["events"] = json!({"onClick":["closeTab",i]});
            let mut tab = view(
                json!({"flex":1,"minWidth":0,"height":28,"flexDirection":"row","alignItems":"center","paddingHorizontal":6,"gap":4,"backgroundColor":if i==self.active{color(self,"windowBg","#202025")}else{"transparent".into()},"borderRight":format!("1px solid {}",color(self,"separator","#38383e"))}),
                vec![
                    close,
                    file_label(
                        self,
                        title.clone(),
                        title,
                        json!({"flex":1,"minWidth":0,"numberOfLines":1,"textOverflow":"ellipsis","fontSize":12,"fontWeight":if i==self.active{600}else{400},"color":if i==self.active{txt(self)}else{dim(self)},"whiteSpace":"nowrap","overflow":"hidden","textAlign":"center"}),
                    ),
                ],
            );
            tab["events"] = json!({"onClick":["tab",i]});
            tabs.push(tab);
        }
        if end < self.tabs.len() {
            tabs.push(button(json!("›"), json!(["tab", end]), false));
        }
        tabs.push(button(json!("+"), action("newTab"), self.tabs.len() >= 32));
        view(
            json!({"height":28,"flexShrink":0,"flexDirection":"row","alignItems":"center","backgroundColor":color(self,"sidebarBg","#25252b"),"borderBottom":format!("1px solid {}",color(self,"separator","#38383e"))}),
            tabs,
        )
    }
    fn path_bar(&self) -> Value {
        let mut children = Vec::new();
        if let Some(path) = self.tab().location.path() {
            let mut ancestors: Vec<_> = path.ancestors().collect();
            ancestors.reverse();
            if path.starts_with(&self.policy.home) {
                ancestors.retain(|p| p.starts_with(&self.policy.home));
            }
            // Keep the current folder and its nearest parents visible in small windows.
            let capacity = (((self.width - self.side_width - 5.) / 130.).floor() as usize).max(1);
            for p in ancestors
                .iter()
                .skip(ancestors.len().saturating_sub(capacity))
            {
                if !children.is_empty() {
                    children.push(label(json!("›"), json!({"fontSize":12,"color":dim(self)})));
                }
                let mut crumb = view(
                    json!({"flexDirection":"row","alignItems":"center","gap":4,"flexShrink":1,"minWidth":0}),
                    vec![
                        icon_ref("folder", 13.),
                        label(
                            self.location_title(&Location::Directory(p.to_path_buf())),
                            json!({"fontSize":11,"color":txt(self),"whiteSpace":"nowrap","overflow":"hidden"}),
                        ),
                    ],
                );
                if let Some(path) = p.to_str() {
                    crumb["events"] = json!({"onClick":["navigate",path]});
                }
                children.push(crumb);
            }
        } else {
            children.push(label(
                self.title_value(),
                json!({"fontSize":11,"color":dim(self)}),
            ));
        }
        let mut bar = view(
            json!({"height":26,"flexShrink":0,"flexDirection":"row","alignItems":"center","gap":5,"paddingHorizontal":10,"overflow":"hidden","borderTop":format!("1px solid {}",color(self,"separator","#38383e"))}),
            children,
        );
        bar["events"] = json!({"onDoubleClick":["goTo"]});
        bar
    }
    fn file_cell(
        &self,
        e: &Entry,
        i: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        icons: bool,
        shared: &Shared,
    ) -> Value {
        let shown_name = if !e.dir
            && !shared.preferences["showExtensions"]
                .as_bool()
                .unwrap_or(true)
        {
            std::path::Path::new(&e.name)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            e.name.clone()
        };
        let selected = self.tab().selected.contains(&i);
        let fg = if selected {
            "#ffffff".to_owned()
        } else {
            txt(self)
        };
        let content = if icons {
            vec![
                entry_icon(self, e, 56. * self.zoom, true),
                label(
                    json!(shown_name),
                    json!({"fontSize":(12.*self.zoom).max(10.),"lineHeight":1.3,"color":fg,"textAlign":"center","width":(width-14.).max(1.),"height":((12.*self.zoom).max(10.)*2.6+2.).ceil(),"numberOfLines":2,"textOverflow":"ellipsis","overflow":"hidden"}),
                ),
            ]
        } else {
            let mut c = vec![
                entry_icon(self, e, 22. * self.zoom, false),
                label(
                    json!(shown_name),
                    json!({"fontSize":(13.*self.zoom).max(11.),"color":fg,"flex":1,"minWidth":0,"whiteSpace":"nowrap","numberOfLines":1,"textOverflow":"ellipsis","overflow":"hidden"}),
                ),
            ];
            if width > 400. * self.zoom {
                c.push(label(
                    json!(if e.dir { String::new() } else { bytes(e.size) }),
                    json!({"fontSize":12.*self.zoom,"color":dim(self),"width":82.*self.zoom,"textAlign":"right"}),
                ));
            }
            if width > 560. * self.zoom {
                c.push(label(
                    json!({"format":"date","value":e.mtime}),
                    json!({"fontSize":11.*self.zoom,"color":dim(self),"width":135.*self.zoom}),
                ));
            }
            if let Some(tags) = shared.tags.get(&e.path) {
                for tag in tags.iter().take(3) {
                    let hex = match tag.as_str() {
                        "red" => "#ff6257",
                        "orange" => "#ff9d33",
                        "yellow" => "#e0c947",
                        "green" => "#53bf70",
                        "purple" => "#af7bf4",
                        _ => "#479eff",
                    };
                    c.push(view(
                        json!({"width":7,"height":7,"borderRadius":4,"backgroundColor":hex}),
                        vec![],
                    ));
                }
            }
            c
        };
        let mut cell = view(
            json!({"position":"absolute","left":x,"top":y,"width":width,"height":height,"paddingHorizontal":7,"gap":if icons{5.*self.zoom}else{8.*self.zoom},"flexDirection":if icons{"column"}else{"row"},"alignItems":"center","justifyContent":if icons{"center"}else{"flex-start"},"borderRadius":6,"backgroundColor":if selected{color(self,"accent","#1485f8")}else{"transparent".into()}}),
            content,
        );
        cell["key"] = json!(format!("entry:{}", e.path));
        cell["type"] = json!("fileTile");
        cell["tooltip"] = json!(e.name);
        cell["tooltipLayer"] = json!(if self.mode == Mode::Browser {
            "tooltip"
        } else {
            "system"
        });
        cell["events"] = json!({"onClick":["select",i],"onDoubleClick":["activate",i],"onContextMenu":["context",i]});
        if self.mode == Mode::Browser {
            cell["events"]["onMouseDown"] = json!(["dragStart", i]);
            cell["events"]["onMouseUp"] = json!(["drop", i]);
        }
        cell
    }
    fn entries_view(&self, shared: &Shared) -> Value {
        let items = self.filtered();
        let past = self.past_columns();
        let width = self.entry_width();
        let height = self.entry_height();
        let icons = self.tab().view == "icons";
        let gallery = self.tab().view == "gallery";
        let columns = self.tab().view == "columns";
        let preview_height = self.gallery_height();
        let (count, cell_width, row_height) = self.entry_grid();
        let rows = items.len().div_ceil(count);
        let content_h = rows as f64 * row_height;
        let area_h = self.entry_area();
        let offset = self.tab().offset.min((content_h - area_h).max(0.));
        let start = (offset / row_height).floor() as usize;
        let visible = (area_h / row_height).ceil() as usize + 2;
        let mut cells = Vec::new();
        if gallery {
            if let Some(e) = self.selected_entries().next() {
                cells.push(view(json!({"position":"absolute","left":0,"top":0,"width":width,"height":preview_height,"alignItems":"center","justifyContent":"center","backgroundColor":color(self,"sidebarBg","#25252b")}),vec![entry_icon(self, e,(preview_height-40.).clamp(1.,240.),true),file_label(self,json!(e.name),json!(e.name),json!({"fontSize":13.*self.zoom,"numberOfLines":1,"textOverflow":"ellipsis","color":txt(self)}))]));
            }
        }
        // Cells stay at fixed positions inside one moving container, so scrolling
        // changes a single offset and only the rows entering the view are new.
        let mut visible_cells = Vec::new();
        let mut keys = HashSet::new();
        for row in start..(start + visible).min(rows) {
            for column in 0..count {
                if let Some(index) = items.get(row * count + column) {
                    let e = &self.tab().entries[*index];
                    let mut cell = self.file_cell(
                        e,
                        *index,
                        column as f64 * cell_width,
                        row as f64 * row_height + preview_height,
                        cell_width,
                        row_height,
                        icons,
                        shared,
                    );
                    // A virtual location can repeat a path; React keys cannot repeat.
                    if !keys.insert(e.path.as_str()) {
                        cell.as_object_mut().unwrap().remove("key");
                    }
                    visible_cells.push(cell);
                }
            }
        }
        cells.push(view(
            json!({"position":"absolute","left":0,"top":-offset,"width":width,"height":content_h+preview_height}),
            visible_cells,
        ));
        if items.is_empty() {
            cells.push(view(json!({"position":"absolute","left":0,"top":0,"width":width,"height":height,"alignItems":"center","justifyContent":"center"}),vec![label(if self.visibly_loading(){tr("files.rust.loading")}else{tr("files.empty")},json!({"fontSize":13,"color":dim(self)}))]));
        }
        if content_h > area_h {
            let thumb = (area_h * area_h / content_h).max(24.).min(area_h);
            let y = preview_height + offset / (content_h - area_h) * (area_h - thumb);
            let mut handle = view(
                json!({"position":"absolute","right":1,"top":y,"width":10,"height":thumb,"borderRadius":4,"backgroundColor":"rgba(150,150,165,0.55)"}),
                vec![],
            );
            handle["events"] = json!({"onMouseDown":["scrollDrag"],"onClick":["noop"]});
            cells.push(handle);
        }
        let mut list = view(
            json!({"flex":1,"overflow":"hidden","backgroundColor":color(self,"windowBg","#202025")}),
            cells,
        );
        list["events"] =
            json!({"onWheel":["wheel"],"onContextMenu":["context",null],"onClick":["clear"]});
        if self.mode == Mode::Browser {
            list["events"]["onMouseUp"] = json!(["drop", null]);
        }
        if columns && !self.tab().columns.is_empty() {
            let mut cols = Vec::new();
            let column_width = self.column_width();
            let row_height = self.column_row();
            for (path, entries) in self.tab().columns.iter().rev().take(past).rev() {
                let mut children = vec![label(
                    json!(path.file_name().unwrap_or_default().to_string_lossy()),
                    json!({"position":"absolute","left":0,"top":0,"width":column_width-1.,"height":row_height,"fontSize":(11.*self.zoom).max(10.),"color":dim(self),"paddingHorizontal":8.*self.zoom,"paddingVertical":4.*self.zoom,"whiteSpace":"nowrap","textOverflow":"ellipsis","backgroundColor":color(self,"windowBg","#202025"),"zIndex":1}),
                )];
                let entries: Vec<_> = entries
                    .iter()
                    .filter(|e| {
                        (self.hidden || !e.name.starts_with('.')) && e.raw.to_str().is_some()
                    })
                    .collect();
                let area = (height - row_height).max(1.);
                let offset = self
                    .tab()
                    .column_offsets
                    .get(path)
                    .copied()
                    .unwrap_or(0.)
                    .min((entries.len() as f64 * row_height - area).max(0.));
                let start = (offset / row_height).floor() as usize;
                let count = (area / row_height).ceil() as usize + 1;
                for (i, e) in entries.iter().enumerate().skip(start).take(count) {
                    let mut item = side_item(
                        self,
                        json!(e.name),
                        json!(["navigate", if e.dir { e.raw.clone() } else { path.clone() }]),
                        false,
                        if e.dir { "folder" } else { "file" },
                    );
                    item["style"]["position"] = json!("absolute");
                    item["style"]["top"] = json!(row_height + i as f64 * row_height - offset);
                    item["style"]["width"] = json!(column_width - 1.);
                    item["style"]["height"] = json!(row_height);
                    item["children"][0] =
                        icon(if e.dir { "folder" } else { "file" }, 19. * self.zoom);
                    item["children"][1]["style"]["fontSize"] = json!((13. * self.zoom).max(11.));
                    if !e.dir {
                        item["events"] = json!({"onDoubleClick":["openRaw",e.raw]});
                    }
                    children.push(item);
                }
                let mut col = view(
                    json!({"width":column_width,"flexShrink":0,"overflow":"hidden","borderRight":format!("1px solid {}",color(self,"separator","#38383e"))}),
                    children,
                );
                col["events"] = json!({"onWheel":["columnWheel",path]});
                cols.push(col);
            }
            cols.push(list);
            view(
                json!({"flex":1,"flexDirection":"row","overflow":"hidden"}),
                cols,
            )
        } else {
            list
        }
    }
    /// Separate Files-owned window, so browsing stays available during copies.
    pub fn transfers_view(&self) -> Value {
        let mut cards = Vec::new();
        for job in self.jobs().as_array().into_iter().flatten() {
            let done = job["done"] == true;
            let failed = job["failed"] == true;
            let cancelled = job["cancel"] == true;
            let phase = job["phase"].as_str().unwrap_or("scanning");
            let copied = job["bytes"].as_u64().unwrap_or(0);
            let total = job["total"].as_u64().unwrap_or(0);
            let percent = transfer_percent(job);
            let status = if cancelled && !done {
                "files.transfer.cancelling"
            } else {
                match phase {
                    "scanning" => "files.transfer.scanning",
                    "syncing" => "files.transfer.syncing",
                    "complete" => "files.rust.completed",
                    "cancelled" => "files.transfer.cancelled",
                    "failed" => "files.transfer.failed",
                    "moving" => "files.transfer.moving",
                    _ if job["move"] == true => "files.transfer.moving",
                    _ => "files.transfer.copying",
                }
            };
            let accent = if failed && !cancelled {
                "#e66161".into()
            } else {
                color(self, "accent", "#3584e4")
            };
            let text_style = json!({"fontSize":12,"color":dim(self),"whiteSpace":"nowrap","textOverflow":"ellipsis","overflow":"hidden","numberOfLines":1,"minWidth":0});
            let mut content = vec![
                view(
                    json!({"flexDirection":"row","alignItems":"center","gap":12}),
                    vec![
                        label(
                            tr(status),
                            json!({"fontSize":14,"fontWeight":600,"color":txt(self),"flex":1}),
                        ),
                        label(
                            if phase == "scanning" {
                                json!("…")
                            } else {
                                json!(format!("{percent}%"))
                            },
                            json!({"fontSize":22,"fontWeight":600,"color":txt(self)}),
                        ),
                    ],
                ),
                label(
                    json!({"key":"files.transfer.from","params":{"path":job["src"]}}),
                    text_style.clone(),
                ),
                label(
                    json!({"key":"files.transfer.to","params":{"path":job["dst"]}}),
                    text_style.clone(),
                ),
                view(
                    json!({"height":9,"borderRadius":5,"backgroundColor":color(self,"separator","#45454c"),"overflow":"hidden","flexShrink":0}),
                    vec![view(
                        json!({"width":format!("{percent}%"),"height":9,"borderRadius":5,"backgroundColor":accent}),
                        vec![],
                    )],
                ),
                label(
                    json!({"key":"files.transfer.size","params":{"done":bytes(copied),"total":bytes(total)}}),
                    text_style.clone(),
                ),
                label(
                    json!({"key":"files.transfer.files","params":{"done":job["files"],"total":job["total_files"]}}),
                    text_style.clone(),
                ),
            ];
            if let Some(name) = job["current"].as_str().filter(|s| !s.is_empty()) {
                content.push(label(json!(name), text_style.clone()));
            }
            if !done && phase != "scanning" {
                let rate = job["bytes_per_second"].as_f64().unwrap_or(0.).max(0.) as u64;
                content.push(label(
                    json!({"key":"files.transfer.speed","params":{"speed":bytes(rate)}}),
                    text_style,
                ));
            }
            if failed && !cancelled {
                content.push(label(
                    json!({"error":job["error"]}),
                    json!({"fontSize":12,"color":"#e66161"}),
                ));
            }
            content.push(view(
                json!({"flexDirection":"row","justifyContent":"flex-end"}),
                vec![button(
                    tr(if done {
                        "files.rust.dismiss"
                    } else {
                        "shell.picker.cancel"
                    }),
                    json!([if done { "jobForget" } else { "jobCancel" }, job["id"]]),
                    cancelled && !done,
                )],
            ));
            let mut card = view(
                json!({"padding":18,"gap":9,"borderRadius":10,"backgroundColor":color(self,"control","#303037"),"border":format!("1px solid {}",color(self,"separator","#45454c")),"flexShrink":0}),
                content,
            );
            card["key"] = json!(format!("transfer-{}", job["id"]));
            cards.push(card);
        }
        if cards.is_empty() {
            cards.push(label(
                tr("files.transfer.empty"),
                json!({"fontSize":13,"color":dim(self),"padding":24}),
            ));
        }
        view(
            json!({"flex":1,"minHeight":0,"backgroundColor":color(self,"windowBg","#202025")}),
            vec![
                node(
                    "scroll",
                    json!({"flex":1,"minHeight":0}),
                    vec![view(json!({"padding":14,"gap":12}), cards)],
                ),
                label(
                    tr("files.transfer.hint"),
                    json!({"fontSize":11,"color":dim(self),"paddingHorizontal":18,"paddingVertical":10,"flexShrink":0}),
                ),
            ],
        )
    }
    fn footer(&self) -> Value {
        let shown = self.filtered().len();
        let selected = self.tab().selected.len();
        let mut children = vec![label(
            json!({"key":"files.info.items","params":{"count":shown}}),
            json!({"fontSize":11,"color":dim(self)}),
        )];
        if selected > 0 {
            children.push(label(
                json!({"key":"files.rust.selected","params":{"count":selected}}),
                json!({"fontSize":11,"color":dim(self)}),
            ));
        }
        children.push(view(json!({"flex":1}), vec![]));
        if self.visibly_loading() || self.busy {
            children.push(label(
                tr("files.rust.loading"),
                json!({"fontSize":11,"color":dim(self)}),
            ));
        }
        if let Some(jobs) = self.jobs().as_array().filter(|jobs| !jobs.is_empty()) {
            let active = jobs.iter().filter(|j| j["done"] == false).count();
            children.push(button(
                json!({"key":"files.transfer.summary","params":{"count":active}}),
                action("showTransfers"),
                false,
            ));
        }
        view(
            json!({"height":27,"flexShrink":0,"paddingHorizontal":10,"flexDirection":"row","alignItems":"center","gap":10,"borderTop":format!("1px solid {}",color(self,"separator","#38383e"))}),
            children,
        )
    }
    fn selection_footer(&self) -> Value {
        let mut children = Vec::new();
        if self.mode == Mode::Save {
            children.push(input(&self.name, "name", tr("files.rust.filename")));
        } else {
            children.push(view(json!({"flex":1}), vec![]));
        }
        children.push(button(tr("shell.picker.cancel"), action("cancel"), false));
        children.push(button(
            tr(match self.mode {
                Mode::Save => "files.rust.save",
                Mode::Folder => "files.rust.chooseFolder",
                _ => "files.menu.open",
            }),
            action("choose"),
            self.mode == Mode::Open && self.tab().selected.is_empty(),
        ));
        view(
            json!({"height":50,"flexShrink":0,"paddingHorizontal":12,"flexDirection":"row","alignItems":"center","gap":8,"borderTop":format!("1px solid {}",color(self,"separator","#38383e"))}),
            children,
        )
    }
    fn context_menu(&self, shared: &Shared) -> Option<Value> {
        let (x, y) = self.context?;
        let selected = !self.tab().selected.is_empty();
        let single = self.tab().selected.len() == 1;
        let file = self.selected_entries().next();
        let item = |key: &str, command: &str| json!({"label":tr(key),"action":[command]});
        let mut choices = Vec::new();
        let mut section = |items: Vec<Value>| {
            if !items.is_empty() {
                if !choices.is_empty() {
                    choices.push(json!("-"));
                }
                choices.extend(items);
            }
        };
        if selected {
            let mut open = vec![item("files.menu.open", "open")];
            if single && file.is_some_and(|e| !e.dir) {
                open.push(item("files.menu.quickLook", "preview"));
            }
            if single && file.is_some_and(|e| !e.dir) {
                let mime = match kind(&file.unwrap().name) {
                    "image" => "image/",
                    "text" => "text/",
                    "pdf" => "application/pdf",
                    "video" => "video/",
                    "audio" => "audio/",
                    _ => "",
                };
                let apps: Vec<_> = shared
                    .apps
                    .iter()
                    .filter(|app| {
                        matches!(
                            (app["id"].as_str().unwrap_or(""), mime),
                            ("editor", "text/")
                                | ("photos", "image/")
                                | ("pdf", "application/pdf")
                                | ("player", "video/")
                                | ("player", "audio/")
                        ) || (!mime.is_empty()
                            && app["mimeTypes"].as_array().is_some_and(|types| {
                                types
                                    .iter()
                                    .any(|t| t.as_str().is_some_and(|t| t.starts_with(mime)))
                            }))
                    })
                    .take(8)
                    .map(|app| json!({"label":app["name"],"action":["openWith",app["desktopId"]]}))
                    .collect();
                if !apps.is_empty() {
                    open.push(json!({"label":tr("files.menu.openWith"),"submenu":apps}));
                }
            }
            open.push(item("files.menu.getInfo", "info"));
            section(open);
        }
        if self.mode == Mode::Browser {
            if self.tab().location == Location::Trash {
                let mut trash = Vec::new();
                if selected {
                    trash.extend([
                        item("files.menu.restore", "restore"),
                        item("files.menu.deleteNow", "delete"),
                    ]);
                }
                if !self.tab().entries.is_empty() {
                    trash.push(item("files.menu.emptyTrash", "emptyTrash"));
                }
                section(trash);
            } else {
                let mut edit = Vec::new();
                if selected {
                    edit.push(item("files.menu.copy", "copy"));
                }
                if selected && self.can_write {
                    edit.push(item("files.menu.cut", "cut"));
                }
                if !shared.clipboard.is_empty() && self.can_write {
                    edit.push(item("files.menu.paste", "paste"));
                }
                if single && self.can_write {
                    edit.push(item("files.menu.rename", "rename"));
                }
                if selected && self.can_write {
                    edit.extend([
                        item("files.menu.duplicate", "duplicate"),
                        item("files.menu.trash", "trash"),
                    ]);
                }
                if !selected && self.can_write {
                    edit.push(item("files.menu.newFolder", "newFolder"));
                }
                section(edit);
                let mut tools = Vec::new();
                if selected && self.can_write {
                    tools.push(item("files.rust.compress", "compress"));
                }
                if selected && self.can_write && self.selected_entries().any(|e| archive(&e.name)) {
                    tools.push(item("files.rust.extract", "extract"));
                }
                if selected {
                    tools.push(item("files.menu.copyPath", "copyPath"));
                }
                if self.tab().location.path().is_some() {
                    tools.push(item("files.menu.openTerminal", "terminal"));
                }
                if !selected {
                    if let Some(path) = self.tab().location.path().filter(|p| {
                        **p != self.policy.home && !self.places.iter().any(|place| place.path == *p)
                    }) {
                        let pinned = shared.preferences["favoriteFolders"]
                            .as_array()
                            .is_some_and(|items| {
                                items.iter().any(|i| i["path"].as_str() == path.to_str())
                            });
                        tools.push(item(
                            if pinned {
                                "files.pin.remove"
                            } else {
                                "files.pin.add"
                            },
                            "pin",
                        ));
                    }
                }
                section(tools);
                if selected {
                    let tags: Vec<_> = [
                        "red", "orange", "yellow", "green", "blue", "purple", "gray",
                    ]
                    .into_iter()
                    .map(|c| json!({"label":tr(&format!("files.tag.{c}")),"action":["tag",c]}))
                    .collect();
                    section(vec![json!({"label":tr("files.menu.tags"),"submenu":tags})]);
                }
            }
        }
        if !selected {
            let mut views: Vec<_> = ["icons","list","columns","gallery"].into_iter().map(|v|json!({"label":tr(&format!("files.view.{v}")),"checked":self.tab().view==v,"action":["view",v]})).collect();
            views.push(json!("-"));
            views.push(json!({"label":tr("files.menu.showHidden"),"checked":self.hidden,"action":["hidden"]}));
            section(vec![
                json!({"label":tr("files.menu.view"),"submenu":views}),
                item("files.menu.goToFolder", "goTo"),
            ]);
        }
        if choices.is_empty() {
            return None;
        }
        Some(
            json!({"type":"contextMenu","x":x,"y":y,"items":choices,"layer":if self.mode==Mode::Browser {"popover"} else {"system"}}),
        )
    }
    fn dialog_view(&self, shared: &Shared) -> Option<Value> {
        let dialog = self.dialog.as_ref()?;
        let mut content = Vec::new();
        let mut title = tr("files.menu.getInfo");
        let mut buttons = vec![button(
            tr("shell.picker.cancel"),
            action("cancelDialog"),
            false,
        )];
        let mut wide = false;
        let preview_width = (self.width - 100.).min(610.).max(1.);
        let preview_height = (self.height - 180.).clamp(32., 300.);
        match dialog {
            Dialog::Text {
                title: t,
                value,
                action: a,
                paths,
            } => {
                title = tr(t);
                let mut field = input(value, "dialogText", json!(""));
                field["key"] = json!(format!("dialog-{}", self.dialog_epoch));
                if a == "rename" {
                    // only the name selected at first: typing keeps the extension (the field
                    // reads it once, when the dialog opens)
                    let folder = paths.first().is_some_and(|p| p.is_dir());
                    field["select"] = json!([0, crate::text_edit::stem_len(value, folder)]);
                }
                content.push(field);
                if a == "connect" {
                    for uri in shared.preferences["servers"]["history"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .take(5)
                    {
                        let mut b = button(uri.clone(), action("dialogText"), false);
                        b["value"] = uri.clone();
                        b["action"] = json!(["recentServer", uri]);
                        content.push(b);
                    }
                }
                buttons.push(button(
                    tr("files.rust.confirm"),
                    action("submit"),
                    value.trim().is_empty(),
                ));
            }
            Dialog::Confirm { action: a, paths } => {
                title = if a == "extension" {
                    tr("files.rust.extensionTitle")
                } else if a == "overwrite" {
                    tr("files.rust.overwriteTitle")
                } else if a == "empty" {
                    tr("files.confirm.empty")
                } else {
                    json!({"key":"files.confirm.delete","params":{"count":paths.len()}})
                };
                content.push(label(
                    if a == "extension" {
                        tr("files.rust.extensionBody")
                    } else if a == "overwrite" {
                        tr("files.rust.overwriteBody")
                    } else {
                        tr("files.confirm.cantUndo")
                    },
                    json!({"color":dim(self),"fontSize":13,"lineHeight":1.4}),
                ));
                buttons.push(button(tr("files.rust.confirm"), action("confirm"), false));
            }
            Dialog::Info {
                entries,
                size,
                comment,
            } => {
                wide = true;
                if let Some(e) = entries.first() {
                    title = json!(e.name);
                    content.push(entry_icon(self, e, 50., false));
                    for (k, v) in [
                        (
                            "files.info.where",
                            json!(e
                                .raw
                                .parent()
                                .unwrap_or(&self.policy.home)
                                .to_string_lossy()),
                        ),
                        (
                            "files.info.size",
                            size.map(|s| json!(bytes(s)))
                                .unwrap_or_else(|| tr("files.info.calculating")),
                        ),
                        (
                            "files.info.modified",
                            json!({"format":"date","value":e.mtime}),
                        ),
                        ("files.info.perms", json!(format!("{:o}", e.mode & 0o777))),
                    ] {
                        content.push(view(
                            row(),
                            vec![
                                label(tr(k), json!({"width":120,"fontSize":12,"color":dim(self)})),
                                label(v, json!({"flex":1,"fontSize":12,"color":txt(self)})),
                            ],
                        ));
                    }
                    if let Some(original) = &e.original {
                        content.push(label(
                            json!(original),
                            json!({"fontSize":12,"color":dim(self)}),
                        ));
                    }
                    content.push(input(comment, "comment", tr("files.info.addComment")));
                    let mut tags = Vec::new();
                    for color in ["red", "orange", "yellow", "green", "blue", "purple", "gray"] {
                        tags.push(button(
                            shared.preferences["tagNames"][color]
                                .as_str()
                                .map(|v| json!(v))
                                .unwrap_or_else(|| tr(&format!("files.tag.{color}"))),
                            json!(["tag", color]),
                            false,
                        ));
                    }
                    content.push(view(
                        json!({"flexDirection":"row","flexWrap":"wrap","gap":5}),
                        tags,
                    ));
                }
            }
            Dialog::Preview {
                entry,
                text,
                image,
                media,
            } => {
                wide = true;
                title = json!(entry.name);
                if let Some(id) = media {
                    content.push(
                        json!({"type":"media","media":id,"style":{"height":preview_height,"width":preview_width}}),
                    );
                } else if let Some(image) = image {
                    content.push(json!({"type":"documentImage","image":image,"style":{"height":preview_height,"width":preview_width}}));
                } else if kind(&entry.name) == "image" {
                    content.push(view(json!({"height":preview_height,"width":preview_width,"alignItems":"center","justifyContent":"center"}),vec![entry_icon(self, entry,preview_height.min(preview_width),true)]));
                } else if let Some(text) = text {
                    content.push(node("scroll",json!({"height":preview_height,"width":preview_width}),vec![label(json!(text),json!({"fontSize":12,"color":txt(self),"fontFamily":"monospace","lineHeight":1.4}))]));
                } else {
                    content.push(entry_icon(self, entry, 96., false));
                    content.push(label(
                        json!(bytes(entry.size)),
                        json!({"fontSize":12,"color":dim(self)}),
                    ));
                }
                buttons.push(button(tr("files.menu.open"), action("open"), false));
            }
            Dialog::Network { value } => {
                wide = true;
                title = tr("files.menu.connect");
                if value["type"] == "question" {
                    content.push(label(
                        value["message"].clone(),
                        json!({"color":txt(self),"fontSize":12,"maxHeight":230}),
                    ));
                    for (i, choice) in value["choices"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        buttons.push(button(choice.clone(), json!(["networkReply", i]), false));
                    }
                } else {
                    content.push(label(
                        tr("files.rust.credentials"),
                        json!({"color":dim(self),"fontSize":12}),
                    ));
                    for (key, caption) in [
                        ("user", "files.net.user"),
                        ("domain", "files.net.domain"),
                        ("password", "files.net.password"),
                    ] {
                        let mut field = input(
                            value[key].as_str().unwrap_or(""),
                            "networkField",
                            tr(caption),
                        );
                        field["action"] = json!(["networkField", key]);
                        field["secure"] = json!(key == "password");
                        field["submit"] = json!(["networkReply", "login"]);
                        field["cancel"] = json!(["cancelDialog"]);
                        content.push(field);
                    }
                    if value["anonymous"] == true {
                        buttons.push(button(
                            tr("files.net.asGuest"),
                            json!(["networkReply", "guest"]),
                            false,
                        ));
                    }
                    buttons.push(button(
                        tr("files.rust.confirm"),
                        json!(["networkReply", "login"]),
                        false,
                    ));
                }
            }
            Dialog::Preferences => {
                wide = true;
                title = tr("files.settings.title");
                for (key, label_key, default) in [
                    ("foldersInTabs", "files.settings.tabs", false),
                    ("showExtensions", "files.settings.extensions", true),
                    ("warnExtension", "files.settings.warnExt", true),
                    ("warnEmptyTrash", "files.settings.warnTrash", true),
                    ("foldersFirst", "files.settings.foldersFirst", true),
                ] {
                    let checked = shared.preferences[key].as_bool().unwrap_or(default);
                    content.push(button(
                        json!({"key":label_key,"prefix":if checked{"✓  "}else{"    "}}),
                        json!(["pref", key]),
                        false,
                    ));
                }
                content.push(label(
                    tr("files.rust.preferencesHint"),
                    json!({"fontSize":12,"color":dim(self)}),
                ));
            }
        }
        // Validation must be visible in the dialog that failed, rather than
        // only in the listing underneath its dimmed overlay.
        if let Some(error) = &self.error {
            content.insert(0,label(json!({"error":error}),json!({"fontSize":12,"color":"#ff8278","lineHeight":1.4,"maxHeight":60,"paddingBottom":6})));
        }
        let mut body = vec![label(
            title,
            json!({"fontSize":17,"fontWeight":600,"color":txt(self),"whiteSpace":"nowrap","overflow":"hidden","flexShrink":0}),
        )];
        body.push(node("scroll",json!({"height":if matches!(dialog, Dialog::Preview {..}) {preview_height} else {(content.len() as f64 * 46.).min((self.height-160.).max(32.))},"flexShrink":1}),content));
        body.push(view(
            json!({"flexDirection":"row","justifyContent":"flex-end","gap":8,"marginTop":8}),
            buttons,
        ));
        let mut panel = view(
            json!({"width":if wide{(self.width-60.).min(650.)}else{(self.width-60.).min(440.)},"maxHeight":self.height-30.,"padding":20,"gap":14,"borderRadius":self.theme["dialogRadius"].as_f64().unwrap_or(12.),"backgroundColor":color(self,"windowBg","#24242a"),"border":format!("1px solid {}",color(self,"separator","#45454c")),"boxShadow":"0 16px 40px rgba(0,0,0,0.5)"}),
            body,
        );
        panel["events"] = json!({"onMouseDown":["noop"],"onClick":["noop"]});
        Some(view(
            json!({"position":"absolute","left":0,"top":0,"width":self.width,"height":self.height,"backgroundColor":"rgba(0,0,0,0.35)","zIndex":200,"alignItems":"center","justifyContent":"center"}),
            vec![panel],
        ))
    }
    pub fn render(&self, shared: &Shared) -> Value {
        let mut main = vec![self.toolbar()];
        if self.search_height() > 0. {
            main.push(view(json!({"height":32,"flexShrink":0,"paddingHorizontal":10,"flexDirection":"row","alignItems":"center","gap":8}),vec![label(tr("files.searchIn"),json!({"fontSize":12,"color":dim(self)})),button(tr(if self.search_home {"files.place.home"} else {"files.settings.scopeHere"}),action("searchScope"),false)]));
        }
        if self.tab().location == Location::Network {
            let status = match self.network_status["state"].as_str().unwrap_or("") {
                "no-lan" => "files.net.noLan",
                "no-helper" => "files.net.noHelper",
                "denied" => "files.net.denied",
                "no-connector" => "files.net.noConnector",
                _ => "files.net.listening",
            };
            if self.discovered.is_empty() {
                main.push(label(
                    tr(status),
                    json!({"fontSize":12,"color":dim(self),"paddingHorizontal":8,"height":28,"flexShrink":0}),
                ));
            }
            main.push(view(
                json!({"height":32,"flexShrink":0}),
                vec![button(
                    tr("files.menu.connect"),
                    action("connect"),
                    self.busy,
                )],
            ));
        }
        if self.tabs_height() > 0. {
            main.push(self.tabs_view());
        }
        if let Some(error) = &self.error {
            main.push(label(
                json!({"error":error}),
                json!({"fontSize":12,"color":"#ff8278","height":44,"flexShrink":0,"padding":8,"lineHeight":1.3,"overflow":"hidden"}),
            ));
        }
        for error in &self.job_errors {
            main.push(view(json!({"height":32,"flexShrink":0,"paddingHorizontal":8,"flexDirection":"row","alignItems":"center","gap":8}),vec![label(json!({"error":error}),json!({"flex":1,"fontSize":12,"color":"#ff8278"})),button(tr("files.rust.dismiss"),json!(["jobForget",0]),false)]));
        }
        main.push(self.entries_view(shared));
        main.push(self.path_bar());
        main.push(self.footer());
        if self.mode != Mode::Browser {
            main.push(self.selection_footer());
        }
        let body = view(
            json!({"flex":1,"flexDirection":"row","overflow":"hidden"}),
            vec![
                self.sidebar(shared),
                {
                    let mut grip = view(json!({"width":5,"flexShrink":0}), vec![]);
                    grip["events"] = json!({"onMouseDown":["resizeSide"]});
                    grip
                },
                view(json!({"flex":1,"overflow":"hidden"}), main),
            ],
        );
        let mut all = Vec::new();
        if self.mode != Mode::Browser {
            all.push(view(json!({"height":42,"flexShrink":0,"paddingHorizontal":14,"justifyContent":"center","borderBottom":format!("1px solid {}",color(self,"separator","#38383e"))}),vec![label(if self.title.is_empty(){tr(match self.mode{Mode::Save=>"files.rust.save",Mode::Folder=>"files.rust.chooseFolder",_=>"files.menu.open"})}else{json!(self.title)},json!({"fontSize":14,"fontWeight":600,"color":txt(self)}))]));
        }
        all.push(body);
        if let Some(drag) = self.drag.as_ref().filter(|d| d["active"] == true) {
            all.push(view(json!({"position":"absolute","left":drag["atX"].as_f64().unwrap_or(0.)-20.,"top":drag["atY"].as_f64().unwrap_or(0.)-20.,"width":48,"height":48,"pointerEvents":"none","opacity":0.8}),vec![icon_ref("files",42.)]));
        }
        if let Some(menu) = self.context_menu(shared) {
            all.push(menu);
        }
        if let Some(dialog) = self.dialog_view(shared) {
            all.push(dialog);
        }
        view(
            json!({"flex":1,"backgroundColor":color(self,"windowBg","#202025"),"overflow":"hidden"}),
            all,
        )
    }
    pub fn menus(&self, shared: &Shared) -> Value {
        let selected = !self.tab().selected.is_empty();
        let writable = self.can_write;
        let item = |key: &str, act: &str, disabled: bool| json!({"label":{"key":key},"action":[act],"disabled":disabled});
        json!([
            {"label":{"key":"files.menu.file"},"items":[item("files.menu.newWindow","newWindow",false),item("files.menu.newTab","newTab",false),item("files.menu.newFolder","newFolder",!writable),"-",item("files.menu.open","open",!selected),item("files.menu.getInfo","info",!selected),item("files.menu.rename","rename",!writable||self.tab().selected.len()!=1),item("files.menu.duplicate","duplicate",!writable||!selected),item("files.menu.trash","trash",!writable||!selected)]},
            {"label":{"key":"files.menu.edit"},"items":[item("files.menu.copy","copy",!selected),item("files.menu.cut","cut",!selected||!writable),item("files.menu.paste","paste",!writable||shared.clipboard.is_empty()),item("files.menu.selectAll","selectAll",false)]},
            {"label":{"key":"files.menu.view"},"items":[{"label":{"key":"files.view.icons"},"action":["view","icons"],"checked":self.tab().view=="icons"},{"label":{"key":"files.view.list"},"action":["view","list"],"checked":self.tab().view=="list"},{"label":{"key":"files.view.columns"},"action":["view","columns"],"checked":self.tab().view=="columns"},{"label":{"key":"files.view.gallery"},"action":["view","gallery"],"checked":self.tab().view=="gallery"},"-",item("files.menu.showHidden","hidden",false),{"label":{"key":"files.rust.sortName"},"action":["sort","name"]},{"label":{"key":"files.rust.sortSize"},"action":["sort","size"]},{"label":{"key":"files.rust.sortDate"},"action":["sort","date"]}]},
            {"label":{"key":"files.menu.go"},"items":[item("files.menu.back","back",self.tab().back.is_empty()),item("files.menu.forward","forward",self.tab().forward.is_empty()),item("files.menu.up","up",self.tab().location.path().is_none()),item("files.menu.goToFolder","goTo",false),item("files.menu.connect","connect",false)]},
            {"label":{"key":"files.settings.title"},"items":[item("files.settings.open","preferences",false)]}
        ])
    }
}

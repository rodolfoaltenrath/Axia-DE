//! What the Axia Store lists. Until its own catalog exists, it shows the apps on
//! this computer: the shell's launcher apps, enriched with the categories and PNG
//! icons of their `.desktop` files, and the Axia packages (`.lpk`) installed.
use crate::applications::DesktopFacts;
use serde_json::Value;
use std::collections::HashMap;
#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    /// A PNG file.
    File(String),
    /// A compiled app that draws its own icon (a native view).
    Compiled(String),
    /// The initial over a colour from the name.
    Letter,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Part of Axia DE.
    System,
    /// An Axia package (`.lpk`).
    Package,
    /// A Linux app (`.desktop`).
    Linux,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The shell's id (`desktop:<id>` for Linux apps).
    pub id: String,
    pub name: String,
    pub summary: String,
    pub categories: Vec<String>,
    pub icon: Icon,
    pub source: Source,
    pub version: String,
}
/// The sidebar's groups after Discover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Create,
    Work,
    Play,
    Develop,
}
impl Group {
    pub const ALL: [Self; 4] = [Self::Create, Self::Work, Self::Play, Self::Develop];
    pub fn id(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Work => "work",
            Self::Play => "play",
            Self::Develop => "develop",
        }
    }
    fn categories(self) -> &'static [&'static str] {
        match self {
            Self::Create => &[
                "Graphics",
                "2DGraphics",
                "3DGraphics",
                "Photography",
                "AudioVideo",
                "Audio",
                "Video",
                "Music",
                "Publishing",
            ],
            Self::Work => &[
                "Office",
                "Productivity",
                "Finance",
                "Calendar",
                "Email",
                "ContactManagement",
                "WordProcessor",
                "Spreadsheet",
                "Presentation",
                "ProjectManagement",
            ],
            Self::Play => &["Game"],
            Self::Develop => &[
                "Development",
                "IDE",
                "Debugger",
                "RevisionControl",
                "TerminalEmulator",
            ],
        }
    }
}
/// The Categories page: an id, the freedesktop categories it gathers and a tile colour.
pub const CATEGORIES: [(&str, &[&str], &str); 9] = [
    (
        "office",
        &["Office", "Productivity", "Finance", "Calendar", "Email"],
        "#f2994a",
    ),
    (
        "graphics",
        &["Graphics", "2DGraphics", "3DGraphics", "Photography"],
        "#bb6bd9",
    ),
    (
        "multimedia",
        &["AudioVideo", "Audio", "Video", "Music"],
        "#eb5757",
    ),
    (
        "internet",
        &["Network", "WebBrowser", "Chat", "InstantMessaging"],
        "#2d9cdb",
    ),
    ("games", &["Game"], "#27ae60"),
    (
        "development",
        &["Development", "IDE", "TerminalEmulator"],
        "#5b6ee1",
    ),
    ("education", &["Education", "Science"], "#f2c94c"),
    (
        "utilities",
        &["Utility", "Accessories", "TextEditor"],
        "#4f8a8b",
    ),
    (
        "system",
        &["System", "Settings", "Monitor", "FileManager"],
        "#8e8e93",
    ),
];
impl Entry {
    pub fn has(&self, list: &[&str]) -> bool {
        self.categories.iter().any(|c| list.contains(&c.as_str()))
    }
    pub fn in_group(&self, g: Group) -> bool {
        self.has(g.categories())
    }
    pub fn in_category(&self, id: &str) -> bool {
        CATEGORIES
            .iter()
            .find(|c| c.0 == id)
            .is_some_and(|c| self.has(c.1))
    }
    /// `query` is already trimmed and lowercased (once per search, not per app).
    pub fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || [&self.name, &self.summary, &self.id]
                .iter()
                .any(|s| s.to_lowercase().contains(query))
    }
}
/// The apps the shell launches (`catalog` facts), one entry each, sorted by name.
/// `lang` picks the language of strings that packages localize.
pub fn collect(
    catalog: &[Value],
    desktop: &[DesktopFacts],
    drawn: &[&str],
    lang: &str,
) -> Vec<Entry> {
    let by_id: HashMap<&str, &DesktopFacts> = desktop.iter().map(|f| (f.id.as_str(), f)).collect();
    let text = |v: &Value| super::catalog_text(v, lang).unwrap_or_default();
    let mut out: Vec<Entry> = catalog
        .iter()
        .filter_map(|app| {
            let id = app["id"].as_str().filter(|s| !s.is_empty())?.to_owned();
            let name = text(&app["name"]);
            if name.is_empty() {
                return None;
            }
            let summary = [&app["comment"], &app["description"], &app["genericName"]]
                .into_iter()
                .map(text)
                .find(|s| !s.is_empty())
                .unwrap_or_default();
            let linux = id.strip_prefix("desktop:");
            let facts = linux.and_then(|d| by_id.get(d).copied());
            let mut categories: Vec<String> = facts
                .map(|f| {
                    f.categories
                        .split(';')
                        .filter(|c| !c.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            if let Some(c) = app["category"].as_str().filter(|c| !c.is_empty()) {
                categories.push(c.to_owned());
            }
            let icon = match facts.map(|f| f.icon.as_str()).filter(|p| !p.is_empty()) {
                Some(path) => Icon::File(path.to_owned()),
                None if drawn.contains(&id.as_str()) => Icon::Compiled(id.clone()),
                None => Icon::Letter,
            };
            let source = if linux.is_some() {
                Source::Linux
            } else if app["package"] == true {
                Source::Package
            } else {
                Source::System
            };
            Some(Entry {
                version: text(&app["version"]),
                id,
                name,
                summary,
                categories,
                icon,
                source,
            })
        })
        .collect();
    out.sort_by_key(|e| e.name.to_lowercase());
    out.dedup_by(|a, b| a.id == b.id);
    out
}

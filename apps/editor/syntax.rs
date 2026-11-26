//! Syntax highlighting: a tokenizer with a stack of states, one line at a time. The state at
//! the end of a line (inside a comment or a multi-line string) starts the next one.
//!
//! Each rule matches at the current position. The few look-aheads of the grammar are a second
//! pattern that must (or must not) match right after, because the `regex` crate has none.
//! Word, digit and boundary classes are ASCII.
use regex::Regex;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Stay,
    Push(&'static str),
    Pop,
}
enum Matcher {
    Pattern {
        re: Regex,
        follow: Option<(Regex, bool)>,
        /// The match must not start in the middle of a word.
        word_start: bool,
    },
    /// Everything up to (not including) the delimiter, which may be missing on this line.
    Until { end: &'static str, escapes: bool },
}
struct Rule {
    matcher: Matcher,
    kind: &'static str,
    next: Next,
}
pub struct Language {
    pub id: &'static str,
    pub name: &'static str,
    extensions: &'static [&'static str],
    names: &'static [&'static str],
    /// The line comment mark ("" when the language has none).
    pub comment: &'static str,
    pub block: Option<(&'static str, &'static str)>,
    states: Vec<(&'static str, Vec<Rule>)>,
}

fn anchored(source: &str) -> Regex {
    Regex::new(&format!(r"\A(?:{source})")).expect("a built-in highlighting pattern")
}
fn rule(source: &str, kind: &'static str) -> Rule {
    Rule { matcher: Matcher::Pattern { re: anchored(source), follow: None, word_start: false }, kind, next: Next::Stay }
}
impl Rule {
    fn push(mut self, state: &'static str) -> Self {
        self.next = Next::Push(state);
        self
    }
    fn pop(mut self) -> Self {
        self.next = Next::Pop;
        self
    }
    fn followed(mut self, source: &str, wanted: bool) -> Self {
        if let Matcher::Pattern { follow, .. } = &mut self.matcher {
            *follow = Some((anchored(source), wanted));
        }
        self
    }
}
fn until(end: &'static str, escapes: bool, kind: &'static str) -> Rule {
    Rule { matcher: Matcher::Until { end, escapes }, kind, next: Next::Stay }
}
/// A list of words.
fn keywords(words: &str, kind: &'static str) -> Rule {
    keywords_flags(words, "", kind)
}
fn keywords_flags(words: &str, flags: &str, kind: &'static str) -> Rule {
    let alternatives = words.split(' ').collect::<Vec<_>>().join("|");
    let mut r = rule(&format!(r"{flags}(?:{alternatives})(?-u:\b)"), kind);
    if let Matcher::Pattern { word_start, .. } = &mut r.matcher {
        *word_start = true;
    }
    r
}

const NUMBER: &str = r"(?:0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|(?:[0-9][0-9_]*\.?[0-9_]*|\.[0-9][0-9_]*)(?:[eE][+-]?[0-9]+)?)[a-zA-Z]*";
const IDENT: &str = r"[A-Za-z_$][0-9A-Za-z_$]*";
const TYPE_CAP: &str = r"[A-Z][0-9A-Za-z_$]*";
const OPERATOR: &str = r"[+\-*/%=&|^!<>~?:]+";
const PUNCT: &str = r"[()\[\]{}.,;@]";
const SPACE: &str = r"\s+";
const DOUBLE: &str = r#""(?:[^"\\]|\\.)*"?"#;
const SINGLE: &str = r"'(?:[^'\\]|\\.)*'?";

fn number() -> Rule {
    rule(NUMBER, "number")
}
fn call() -> Rule {
    rule(IDENT, "function").followed(r"\s*\(", true)
}
fn space() -> Rule {
    rule(SPACE, "text")
}
fn double(kind: &'static str) -> Rule {
    rule(DOUBLE, kind)
}
fn single() -> Rule {
    rule(SINGLE, "string")
}

struct CLike {
    keywords: &'static str,
    types: &'static str,
    constants: &'static str,
    extra: Vec<Rule>,
    templates: bool,
}
fn c_like(spec: CLike) -> Vec<(&'static str, Vec<Rule>)> {
    let mut root = vec![space(), rule("//.*", "comment"), rule(r"/\*", "comment").push("block")];
    root.extend(spec.extra);
    if spec.templates {
        root.push(rule("`", "string").push("template"));
    }
    root.extend([double("string"), single(), number(), keywords(spec.constants, "constant"), keywords(spec.keywords, "keyword")]);
    if !spec.types.is_empty() {
        root.push(keywords(spec.types, "type"));
    }
    root.extend([call(), rule(TYPE_CAP, "type"), rule(IDENT, "text"), rule(OPERATOR, "operator"), rule(PUNCT, "punct")]);
    vec![
        ("root", root),
        ("block", block_comment()),
        (
            "template",
            vec![
                rule(r"[^`\\$]+", "string"),
                rule(r"\\.", "string"),
                rule(r"\$\{[^}]*\}", "variable"),
                rule(r"\$", "string"),
                rule("`", "string").pop(),
            ],
        ),
    ]
}
fn block_comment() -> Vec<Rule> {
    vec![rule(r"[^*]+", "comment"), rule(r"\*/", "comment").pop(), rule(r"\*", "comment")]
}

const JS_KEYWORDS: &str = "break case catch class const continue debugger default delete do else export extends finally for from function if import in instanceof let new of return static super switch this throw try typeof var void while with yield async await get set as interface type enum implements private public protected readonly declare namespace abstract keyof";
const SQL_KEYWORDS: &str = "select from where and or not insert into values update set delete create table drop alter add column index primary key foreign references join left right inner outer full on group by order having limit offset as distinct union all exists in is null like between case when then else end begin commit rollback view trigger default unique check returning with";

fn language(
    id: &'static str,
    name: &'static str,
    extensions: &'static [&'static str],
    comment: &'static str,
    block: Option<(&'static str, &'static str)>,
    states: Vec<(&'static str, Vec<Rule>)>,
) -> Language {
    Language { id, name, extensions, names: &[], comment, block, states }
}
impl Language {
    fn named(mut self, names: &'static [&'static str]) -> Self {
        self.names = names;
        self
    }
}
const C_BLOCK: Option<(&str, &str)> = Some(("/*", "*/"));

static LANGUAGES: LazyLock<Vec<Language>> = LazyLock::new(|| {
    vec![
        language("plain", "Texto", &["txt", "log", ""], "", None, vec![("root", vec![rule(".+", "text")])]),
        language("js", "JavaScript", &["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts"], "//", C_BLOCK, c_like(CLike {
            keywords: JS_KEYWORDS,
            types: "string number boolean any unknown never object symbol bigint",
            constants: "true false null undefined NaN Infinity",
            templates: true,
            extra: vec![
                // a regular expression literal: "/…/flags" followed by what can follow a value
                rule(r"/(?:[^/\\\n*]|\\.)(?:[^/\\\n]|\\.)*/[gimsuyd]*", "regexp").followed(r"\s*(?:[.,;)\]}]|\z)", true),
                rule(r"</?[A-Za-z][0-9A-Za-z_.]*", "tag"),
            ],
        })),
        language("c", "C / C++", &["c", "h", "cc", "cpp", "cxx", "hpp", "hh", "ino"], "//", C_BLOCK, c_like(CLike {
            keywords: "auto break case catch class const constexpr continue default delete do else enum explicit extern for friend goto if inline namespace new noexcept operator private protected public register restrict return sizeof static static_assert struct switch template this throw try typedef typename union using virtual volatile while",
            types: "void char short int long float double signed unsigned bool size_t ssize_t uint8_t uint16_t uint32_t uint64_t int8_t int16_t int32_t int64_t FILE",
            constants: "true false NULL nullptr",
            templates: false,
            extra: vec![rule(r"#\s*[0-9A-Za-z_]+.*", "meta")],
        })),
        language("go", "Go", &["go"], "//", C_BLOCK, c_like(CLike {
            keywords: "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var",
            types: "bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string uint uint8 uint16 uint32 uint64 uintptr any",
            constants: "true false nil iota",
            templates: false,
            extra: vec![rule("`[^`]*`?", "string")],
        })),
        language("rust", "Rust", &["rs"], "//", C_BLOCK, c_like(CLike {
            keywords: "as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait type unsafe use where while",
            types: "i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64 bool char str String Vec Option Result Box",
            constants: "true false None Some Ok Err",
            templates: false,
            extra: vec![rule("[a-z_]+!", "function"), rule(r"'[a-z_]+(?-u:\b)", "type").followed("'", false)],
        })),
        language("python", "Python", &["py", "pyw", "pyi"], "#", None, vec![
            ("root", vec![
                space(),
                rule("#.*", "comment"),
                rule(r#"[rRbBuUfF]{0,2}""""#, "string").push("tdq"),
                rule("[rRbBuUfF]{0,2}'''", "string").push("tsq"),
                rule(&format!("[rRbBuUfF]{{0,2}}{DOUBLE}"), "string"),
                rule(&format!("[rRbBuUfF]{{0,2}}{SINGLE}"), "string"),
                rule(r"@[0-9A-Za-z_.]+", "meta"),
                number(),
                keywords("True False None", "constant"),
                keywords("and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield match case self", "keyword"),
                keywords("int float str bool list dict set tuple bytes object type", "type"),
                call(),
                rule(TYPE_CAP, "type"),
                rule(IDENT, "text"),
                rule(OPERATOR, "operator"),
                rule(PUNCT, "punct"),
            ]),
            ("tdq", vec![until(r#"""""#, true, "string"), rule(r#"""""#, "string").pop()]),
            ("tsq", vec![until("'''", true, "string"), rule("'''", "string").pop()]),
        ]),
        language("shell", "Shell", &["sh", "bash", "zsh", "fish", "bashrc", "zshrc", "profile"], "#", None, vec![("root", vec![
            space(),
            rule("#.*", "comment"),
            double("string"),
            single(),
            rule(r"\$\{[^}]*\}|\$[0-9A-Za-z_@#?*!$-]+|\$\(", "variable"),
            number(),
            keywords("if then else elif fi for while until do done case esac in function return local export readonly declare set unset shift exit source alias break continue select time", "keyword"),
            rule(r"[0-9A-Za-z_.-]+", "variable").followed("=", true),
            rule(r"-{1,2}[0-9A-Za-z_-]+", "constant"),
            rule(IDENT, "text"),
            rule(r"[|&;<>()]+", "operator"),
            rule(".", "text"),
        ])])
        .named(&["Makefile", "makefile", "Dockerfile"]),
        language("json", "JSON", &["json", "jsonc", "json5", "webmanifest"], "//", None, vec![("root", vec![
            space(),
            rule(r#""(?:[^"\\]|\\.)*""#, "attr").followed(r"\s*:", true),
            double("string"),
            number(),
            keywords("true false null", "constant"),
            rule("//.*", "comment"),
            rule(PUNCT, "punct"),
            rule(".", "text"),
        ])]),
        language("markdown", "Markdown", &["md", "markdown", "mdx"], "", None, vec![
            ("root", vec![
                rule(r"#{1,6}\s.*", "heading"),
                rule("```.*", "meta").push("code"),
                rule(r"\s*(?:[-*+]|[0-9]+\.)\s", "keyword"),
                rule(">.*", "comment"),
                rule(r"\*\*[^*]+\*\*|__[^_]+__", "bold"),
                rule(r"\*[^*\s][^*]*\*|_[^_\s][^_]*_", "italic"),
                rule("`[^`]+`", "string"),
                rule(r"!?\[[^\]]*\]\([^)]*\)", "link"),
                rule(r"[^#`*_\[!>]+", "text"),
                rule(".", "text"),
            ]),
            ("code", vec![rule(r"```\s*\z", "meta").pop(), rule(".+", "string")]),
        ]),
        language("html", "HTML", &["html", "htm", "xml", "svg", "xhtml", "vue"], "", Some(("<!--", "-->")), vec![
            ("root", vec![
                rule("<!--", "comment").push("comment"),
                rule(r"<![0-9A-Za-z_][^>]*>", "meta"),
                rule(r"</?[0-9A-Za-z_:-]+", "tag").push("tag"),
                rule(r"&[0-9A-Za-z_]+;", "constant"),
                rule("[^<&]+", "text"),
                rule(".", "text"),
            ]),
            ("tag", vec![
                space(),
                rule("/?>", "tag").pop(),
                rule(r"[0-9A-Za-z_:@.-]+", "attr").followed("=", true),
                rule("=", "operator"),
                double("string"),
                single(),
                rule(r"[0-9A-Za-z_:-]+", "attr"),
                rule(".", "text"),
            ]),
            ("comment", vec![until("-->", false, "comment"), rule("-->", "comment").pop(), rule("-", "comment")]),
        ]),
        language("css", "CSS", &["css", "scss", "less"], "", C_BLOCK, vec![
            ("root", vec![
                space(),
                rule(r"/\*", "comment").push("block"),
                rule("//.*", "comment"),
                rule(r"@[0-9A-Za-z_-]+", "keyword"),
                rule(r"[.#][0-9A-Za-z_-]+", "type"),
                rule(r"[0-9A-Za-z_-]+", "attr").followed(r"\s*:[^;{]*[;}]", true),
                rule(r"#[0-9a-fA-F]{3,8}(?-u:\b)", "number"),
                number(),
                double("string"),
                single(),
                rule(r"[0-9A-Za-z_-]+", "function").followed(r"\(", true),
                rule(r"[0-9A-Za-z_-]+", "text"),
                rule(r"[{}:;,()>+~*]", "punct"),
                rule(".", "text"),
            ]),
            ("block", block_comment()),
        ]),
        language("yaml", "YAML", &["yaml", "yml"], "#", None, vec![("root", vec![
            space(),
            rule("#.*", "comment"),
            rule(r"-{3}|\.{3}", "meta"),
            rule("-", "punct").followed(r"\s", true),
            rule(r"[0-9A-Za-z_.-]+", "attr").followed(r"\s*:(?:\s|\z)", true),
            double("string"),
            single(),
            keywords("true false null yes no on off", "constant"),
            number(),
            rule(r"[&*][0-9A-Za-z_-]+", "variable"),
            rule("[:|>]", "operator"),
            rule(r"[^\s#]+", "string"),
        ])]),
        language("php", "PHP", &["php", "phtml"], "//", C_BLOCK, c_like(CLike {
            keywords: "abstract and array as break callable case catch class clone const continue declare default do echo else elseif empty enddeclare endfor endforeach endif endswitch endwhile extends final finally fn for foreach function global goto if implements include include_once instanceof insteadof interface isset list match namespace new or print private protected public require require_once return static switch throw trait try unset use var while xor yield",
            types: "",
            constants: "true false null TRUE FALSE NULL",
            templates: false,
            extra: vec![rule(r"\$[0-9A-Za-z_]+", "variable"), rule(r"<\?php|\?>", "meta"), rule("#.*", "comment")],
        })),
        language("sql", "SQL", &["sql"], "--", C_BLOCK, vec![
            ("root", vec![
                space(),
                rule("--.*", "comment"),
                rule(r"/\*", "comment").push("block"),
                single(),
                double("attr"),
                number(),
                keywords_flags(SQL_KEYWORDS, "(?i-u)", "keyword"),
                keywords_flags("int integer bigint smallint text varchar char boolean date timestamp numeric decimal real serial json jsonb uuid blob", "", "type"),
                call(),
                rule(IDENT, "text"),
                rule(OPERATOR, "operator"),
                rule(PUNCT, "punct"),
            ]),
            ("block", block_comment()),
        ]),
        language("ini", "INI / TOML", &["ini", "toml", "conf", "cfg", "desktop", "service", "properties", "env"], "#", None, vec![("root", vec![
            space(),
            rule("[#;].*", "comment"),
            rule(r"\[\[?[^\]]*\]\]?", "heading"),
            rule(r"[0-9A-Za-z_.\[\]@-]+", "attr").followed(r"\s*=", true),
            rule("=", "operator"),
            double("string"),
            single(),
            keywords("true false yes no on off", "constant"),
            number(),
            rule(".+", "string"),
        ])])
        .named(&[".gitconfig", ".editorconfig"]),
        language("diff", "Diff", &["diff", "patch"], "", None, vec![("root", vec![
            rule(r"(?:\+\+\+|---) .*", "meta"),
            rule("@@.*", "function"),
            rule(r"\+.*", "inserted"),
            rule("-.*", "deleted"),
            rule("(?:diff|index) .*", "meta"),
            rule(".*", "text"),
        ])]),
    ]
});

pub fn languages() -> &'static [Language] {
    &LANGUAGES
}
pub fn by_id(id: &str) -> &'static Language {
    LANGUAGES.iter().find(|l| l.id == id).unwrap_or(&LANGUAGES[0])
}
/// The language of a file, by its whole name or by its extension (a dot file is its extension).
pub fn for_name(name: &str) -> &'static Language {
    let base = name.rsplit('/').next().unwrap_or("");
    if let Some(l) = LANGUAGES.iter().find(|l| l.names.contains(&base)) {
        return l;
    }
    let extension = match base.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_lowercase(),
        _ => base.strip_prefix('.').map(str::to_lowercase).unwrap_or_default(),
    };
    LANGUAGES
        .iter()
        .find(|l| !extension.is_empty() && l.extensions.contains(&extension.as_str()))
        .unwrap_or(&LANGUAGES[0])
}

pub type Token = (String, &'static str);

fn match_len(matcher: &Matcher, line: &str, at: usize) -> usize {
    let rest = &line[at..];
    match matcher {
        Matcher::Pattern { re, follow, word_start } => {
            if *word_start && line[..at].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                return 0;
            }
            let Some(found) = re.find(rest) else { return 0 };
            if let Some((next, wanted)) = follow {
                if next.is_match(&rest[found.end()..]) != *wanted {
                    return 0;
                }
            }
            found.end()
        }
        Matcher::Until { end, escapes } => {
            let mut used = 0;
            let mut chars = rest.chars();
            while !rest[used..].starts_with(end) {
                let Some(c) = chars.next() else { break };
                if *escapes && c == '\\' {
                    // a backslash takes the next character; alone at the end it stays unmatched
                    let Some(next) = chars.next() else { break };
                    used += next.len_utf8();
                }
                used += c.len_utf8();
            }
            used
        }
    }
}
/// Tokenizes a line from a state ("root", or nested states as "a/b"). Returns the tokens, with
/// neighbors of the same type joined, and the state at the end of the line.
pub fn tokenize(language: &Language, line: &str, state: &str) -> (Vec<Token>, String) {
    let mut stack: Vec<&str> = state.split('/').collect();
    let mut tokens: Vec<Token> = Vec::new();
    let mut push = |text: &str, kind: &'static str| match tokens.last_mut() {
        Some((last, k)) if *k == kind => last.push_str(text),
        _ => tokens.push((text.to_owned(), kind)),
    };
    let (mut at, mut guard) = (0, 0);
    while at < line.len() && guard < 10_000 {
        guard += 1;
        let current = stack.last().copied().unwrap_or("root");
        let rules = &language.states.iter().find(|(n, _)| *n == current).unwrap_or(&language.states[0]).1;
        let found = rules.iter().find_map(|r| {
            let n = match_len(&r.matcher, line, at);
            (n > 0).then_some((n, r))
        });
        match found {
            Some((n, r)) => {
                push(&line[at..at + n], r.kind);
                at += n;
                match r.next {
                    Next::Stay => {}
                    Next::Push(s) => stack.push(s),
                    Next::Pop => {
                        if stack.len() > 1 {
                            stack.pop();
                        }
                    }
                }
            }
            None => {
                let n = line[at..].chars().next().map_or(1, char::len_utf8);
                push(&line[at..at + n], if stack.len() > 1 { rules.first().map_or("text", |r| r.kind) } else { "text" });
                at += n;
            }
        }
    }
    if at < line.len() {
        push(&line[at..], "text");
    }
    (tokens, stack.join("/"))
}

/// The color of a token type (a classic IDE palette).
pub fn color(kind: &str, dark: bool) -> &'static str {
    let (d, l) = match kind {
        "comment" => ("#7f8c98", "#5d6c79"),
        "string" => ("#fc6a5d", "#c41a16"),
        "keyword" => ("#fc5fa3", "#9b2393"),
        "number" | "constant" => ("#d0bf69", "#1c00cf"),
        "type" | "tag" | "heading" => ("#5dd8ff", "#0b4f79"),
        "function" => ("#67b7a4", "#326d74"),
        "punct" => ("#a1a1aa", "#5f5f6b"),
        "regexp" => ("#ff8170", "#d12f1b"),
        "variable" => ("#a167e6", "#6c36a9"),
        "meta" => ("#fd8f3f", "#643820"),
        "attr" => ("#bf8555", "#815f03"),
        "bold" => ("#ffffff", "#000000"),
        "italic" => ("#d4d4d8", "#3f3f46"),
        "link" => ("#6bb3ff", "#0e64c8"),
        "inserted" => ("#7ee787", "#116329"),
        "deleted" => ("#ff7b72", "#a40e26"),
        _ => ("#e4e4e7", "#1f1f24"),
    };
    if dark {
        d
    } else {
        l
    }
}

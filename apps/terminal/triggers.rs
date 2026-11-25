//! What a profile does with the lines a program prints (its triggers) and which profile a session
//! takes from where it is (its rules), without any I/O: the window gives the facts and does what
//! comes back. A port of `runTriggers` and `autoProfile` of the former `state.js`. Regular
//! expressions are Rust's: no look-around or back-references, so such a pattern never matches.
use regex::{Regex, RegexBuilder};
use serde_json::Value;
use std::collections::HashSet;

/// The size limit of one compiled expression: a profile cannot make the window spend its time on one.
const PATTERN_LIMIT: usize = 1 << 20;
/// The color a highlight without a parameter gets as background.
const YELLOW: i64 = 0xffd60a;

/// What a trigger asks the window to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Notify(String),
    Bell,
    Attention(String),
    /// Text typed into the session (a literal `\n` is Enter).
    Send(String),
    Capture(String),
    /// The matched text with its foreground and background (`-1`: the engine's default).
    Highlight { text: String, fg: i64, bg: i64 },
    Mark,
    Title(String),
}

fn compile(pattern: &str) -> Option<Regex> {
    RegexBuilder::new(pattern).size_limit(PATTERN_LIMIT).build().ok()
}
/// The parameter with `\1`, `\2`… replaced by the groups of the match (`\0` is the whole match).
fn fill(param: &str, groups: &regex::Captures) -> String {
    let mut out = String::new();
    let mut chars = param.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek().and_then(|d| d.to_digit(10))) {
            ('\\', Some(i)) => {
                chars.next();
                out.push_str(groups.get(i as usize).map_or("", |m| m.as_str()));
            }
            _ => out.push(c),
        }
    }
    out
}
fn color(text: Option<&str>) -> i64 {
    let hex = text.unwrap_or("").trim_start_matches('#');
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        i64::from_str_radix(hex, 16).unwrap_or(-1)
    } else {
        -1
    }
}
/// The actions the lines on screen fire, and the lines now on screen (the next call's `seen`):
/// a line fires once, while it stays on screen. `stop` ends the triggers of that line.
pub fn evaluate(screen: &str, seen: &HashSet<String>, triggers: &[Value]) -> (Vec<Action>, HashSet<String>) {
    let now: Vec<&str> = {
        let mut unique = HashSet::new();
        screen.split('\n').filter(|l| !l.is_empty()).filter(|l| unique.insert(*l)).collect()
    };
    let compiled: Vec<(Option<Regex>, &Value)> = triggers
        .iter()
        .filter(|t| t["enabled"] != false)
        .map(|t| (compile(t["regex"].as_str().unwrap_or("")), t))
        .collect();
    let mut out = vec![];
    for line in &now {
        if seen.contains(*line) {
            continue;
        }
        for (re, trigger) in &compiled {
            let Some(groups) = re.as_ref().and_then(|re| re.captures(line)) else { continue };
            let raw = trigger["param"].as_str().unwrap_or("");
            let param = fill(raw, &groups);
            let whole = groups.get(0).map_or("", |m| m.as_str());
            let shown = if param.is_empty() { line.trim().to_owned() } else { param.clone() };
            match trigger["action"].as_str().unwrap_or("") {
                "notify" => out.push(Action::Notify(shown)),
                "bell" => out.push(Action::Bell),
                "attention" => out.push(Action::Attention(shown)),
                "send" => out.push(Action::Send(param.replace("\\n", "\r"))),
                "capture" => out.push(Action::Capture(line.trim().to_owned())),
                "highlight" => {
                    // "#text,#background", also written "{#text,#background}"; yellow background by default
                    let spec: String = raw.chars().filter(|c| !c.is_whitespace() && *c != '{' && *c != '}').collect();
                    let mut parts = spec.split(',');
                    let fg = color(parts.next());
                    let bg = if raw.is_empty() { YELLOW } else { color(parts.next()) };
                    out.push(Action::Highlight { text: whole.to_owned(), fg, bg });
                }
                "mark" => out.push(Action::Mark),
                "title" => out.push(Action::Title(if param.is_empty() { whole.to_owned() } else { param })),
                "stop" => break,
                _ => {}
            }
        }
    }
    (out, now.into_iter().map(str::to_owned).collect())
}

/// Where a session is: who runs it, on which machine, in which folder.
pub struct Place<'a> {
    pub user: &'a str,
    pub host: &'a str,
    pub cwd: &'a str,
    pub home: &'a str,
}
/// A wildcard pattern as an anchored expression: `*` is anything (anything but `/` in a path), `**` anything.
fn glob(pattern: &str, path: bool) -> Option<Regex> {
    let mut re = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                re.push_str(".*");
            }
            '*' => re.push_str(if path { "[^/]*" } else { ".*" }),
            c if ".+^${}()|[]\\?".contains(c) => {
                re.push('\\');
                re.push(c);
            }
            c => re.push(c),
        }
    }
    re.push('$');
    compile(&re)
}
/// How well one rule fits the place: 0 when it does not. A user is worth 2, a host 1, a folder 4,
/// so the most specific rule wins. A rule is a path (`~/prod/*`, `/srv/**`), `user@host`, `host`,
/// `user@` or `user@host:/path`.
fn score(rule: &str, at: &Place) -> u32 {
    let rule = rule.trim();
    if rule.is_empty() {
        return 0;
    }
    let (user, host, path) = if rule.starts_with('/') || rule.starts_with('~') {
        (None, None, Some(rule))
    } else {
        let mut parts = rule.split(':');
        let who = parts.next().unwrap_or("");
        let path = parts.next().filter(|p| !p.is_empty());
        match who.split_once('@') {
            Some((u, h)) => (Some(u).filter(|u| !u.is_empty()), Some(h.split('@').next().unwrap_or("")).filter(|h| !h.is_empty()), path),
            None => (None, Some(who).filter(|h| !h.is_empty()), path),
        }
    };
    let mut total = 0;
    if let Some(u) = user {
        if !glob(u, false).is_some_and(|re| re.is_match(at.user)) {
            return 0;
        }
        total += 2;
    }
    if let Some(h) = host {
        if !glob(h, false).is_some_and(|re| re.is_match(at.host)) {
            return 0;
        }
        total += 1;
    }
    if let Some(p) = path {
        let expanded = match p.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", at.home),
            _ => p.to_owned(),
        };
        if at.cwd.is_empty() || !glob(&expanded, true).is_some_and(|re| re.is_match(at.cwd)) {
            return 0;
        }
        total += 4;
    }
    total
}
/// The profile whose rules fit the place best (the first one on a tie), if any rule fits.
pub fn best_profile(profiles: &[Value], at: &Place) -> Option<String> {
    let (mut found, mut best) = (None, 0);
    for profile in profiles {
        for rule in profile["rules"].as_str().unwrap_or("").split('\n') {
            let s = score(rule, at);
            if s > best {
                (best, found) = (s, profile["id"].as_str().map(str::to_owned));
            }
        }
    }
    found
}
/// The user and the host of a session as the rules see them: a `user@host` reported by the shell,
/// or this machine's when there is none.
pub fn who<'a>(reported: &'a str, local_user: &'a str, local_host: &'a str) -> (&'a str, &'a str) {
    let (user, host) = match reported.split_once('@') {
        Some((u, h)) => (Some(u).filter(|u| !u.is_empty()), h),
        None => (None, reported),
    };
    let user = user.unwrap_or(if host.is_empty() || host == local_host { local_user } else { "" });
    (user, if host.is_empty() { local_host } else { host })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(screen: &str, triggers: Value) -> Vec<Action> {
        evaluate(screen, &HashSet::new(), triggers.as_array().unwrap()).0
    }

    #[test]
    fn a_line_fires_the_triggers_that_match_it_with_the_groups_in_the_parameter() {
        let t = json!([
            {"regex":"error: (\\w+)","action":"notify","param":"failed \\1 (\\0)"},
            {"regex":"warn","action":"notify","param":""},
            {"regex":"nothing matches","action":"bell"},
        ]);
        assert_eq!(run("ok\nerror: disk\n  warn here  \n", t), [Action::Notify("failed disk (error: disk)".into()), Action::Notify("warn here".into())]);
    }

    #[test]
    fn a_line_fires_once_while_it_stays_on_screen() {
        let t = json!([{"regex":"done","action":"bell"}]);
        let (first, seen) = evaluate("done\nmore", &HashSet::new(), t.as_array().unwrap());
        assert_eq!(first, [Action::Bell]);
        let (again, seen) = evaluate("done\nmore\nnext", &seen, t.as_array().unwrap());
        assert!(again.is_empty(), "it is still the same line");
        assert_eq!(seen.len(), 3);
        let (gone, seen) = evaluate("other", &seen, t.as_array().unwrap());
        assert!(gone.is_empty() && seen.len() == 1);
        let (back, _) = evaluate("done", &seen, t.as_array().unwrap());
        assert_eq!(back, [Action::Bell], "once it left the screen it can fire again");
    }

    #[test]
    fn disabled_broken_and_unknown_triggers_are_skipped_and_stop_ends_the_line() {
        let t = json!([
            {"regex":"x","action":"bell","enabled":false},
            {"regex":"(","action":"bell"},
            {"regex":"(?<=a)x","action":"bell"},
            {"regex":"x","action":"frobnicate"},
            {"regex":"x","action":"stop"},
            {"regex":"x","action":"mark"},
        ]);
        assert!(run("x", t).is_empty());
        let t = json!([{"regex":"x","action":"mark"},{"regex":"x","action":"stop"},{"regex":"x","action":"bell"}]);
        assert_eq!(run("x", t), [Action::Mark], "what comes after stop does not run for that line");
    }

    #[test]
    fn send_capture_title_and_highlight_build_their_actions() {
        let t = json!([
            {"regex":"Password:","action":"send","param":"hunter2\\n"},
            {"regex":"build (\\d+)","action":"capture"},
            {"regex":"build (\\d+)","action":"title","param":"B\\1"},
            {"regex":"hot","action":"title","param":""},
            {"regex":"hot","action":"highlight"},
            {"regex":"cold","action":"highlight","param":"{#ff0000, #00ff00}"},
            {"regex":"mild","action":"highlight","param":"#112233"},
        ]);
        assert_eq!(
            run("Password: \nbuild 42 ok\nhot\ncold\nmild", t),
            [
                Action::Send("hunter2\r".into()),
                Action::Capture("build 42 ok".into()),
                Action::Title("B42".into()),
                Action::Title("hot".into()),
                Action::Highlight { text: "hot".into(), fg: -1, bg: 0xffd60a },
                Action::Highlight { text: "cold".into(), fg: 0xff0000, bg: 0x00ff00 },
                Action::Highlight { text: "mild".into(), fg: 0x112233, bg: -1 },
            ]
        );
    }

    fn profiles() -> Vec<Value> {
        vec![
            json!({"id":"plain","rules":""}),
            json!({"id":"prod","rules":"~/prod/**\nroot@*"}),
            json!({"id":"box","rules":"devbox\nme@devbox:/srv/*"}),
        ]
    }
    fn at<'a>(user: &'a str, host: &'a str, cwd: &'a str) -> Place<'a> {
        Place { user, host, cwd, home: "/home/me" }
    }

    #[test]
    fn rules_match_paths_hosts_and_users_and_the_most_specific_one_wins() {
        let p = profiles();
        assert_eq!(best_profile(&p, &at("me", "laptop", "/home/me/prod/api/src")).as_deref(), Some("prod"));
        assert_eq!(best_profile(&p, &at("me", "laptop", "/home/me/dev")), None);
        assert_eq!(best_profile(&p, &at("root", "laptop", "/tmp")).as_deref(), Some("prod"), "a user rule needs no folder");
        assert_eq!(best_profile(&p, &at("me", "devbox", "/home/me")).as_deref(), Some("box"));
        assert_eq!(best_profile(&p, &at("me", "devbox", "/srv/app")).as_deref(), Some("box"));
        assert_eq!(best_profile(&p, &at("root", "devbox", "/home/me/prod/x")).as_deref(), Some("prod"), "user+folder beat host alone");
        assert_eq!(best_profile(&p, &at("me", "devbox", "")).as_deref(), Some("box"), "a path rule never matches without a folder");
        assert_eq!(best_profile(&[json!({"id":"a","rules":"/srv/*"})], &at("me", "x", "/srv/a/b")), None, "* stops at a slash in a path");
        assert_eq!(best_profile(&[json!({"id":"a","rules":"/srv/**"})], &at("me", "x", "/srv/a/b")).as_deref(), Some("a"));
        assert_eq!(best_profile(&[json!({"id":"a","rules":"a.b"})], &at("me", "axb", "")), None, "a dot is a dot");
    }

    #[test]
    fn the_session_user_and_host_come_from_what_the_shell_reports() {
        assert_eq!(who("", "me", "laptop"), ("me", "laptop"));
        assert_eq!(who("laptop", "me", "laptop"), ("me", "laptop"));
        assert_eq!(who("box", "me", "laptop"), ("", "box"), "another machine: the user is unknown");
        assert_eq!(who("root@box", "me", "laptop"), ("root", "box"));
    }
}

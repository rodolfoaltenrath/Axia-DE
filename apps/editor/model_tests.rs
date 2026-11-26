//! The document, the editing commands, the highlighter and the search.
use super::buffer::{pos, Cursor, Doc, Kind, Pos};
use super::edit::{self, Metrics, Style};
use super::{search, syntax};

fn style(language: &str) -> Style<'static> {
    Style {
        language: syntax::by_id(language),
        tab: 2,
        use_tabs: false,
        page: 10,
        metrics: Metrics { cell: 10., measure: |t| t.chars().count() as f64 * 20. },
    }
}
/// A document with a cursor at each `|`; `[` and `]` mark one selection (head at `]`).
fn doc(marked: &str) -> Doc {
    let mut text = String::new();
    let (mut cursors, mut anchor) = (Vec::new(), None);
    let (mut l, mut c) = (0, 0);
    for ch in marked.chars() {
        match ch {
            '|' => cursors.push(Cursor::at(pos(l, c))),
            '«' => anchor = Some(pos(l, c)),
            '»' => cursors.push(Cursor::range(anchor.take().expect("an opening mark"), pos(l, c))),
            _ => {
                text.push(ch);
                if ch == '\n' {
                    l += 1;
                    c = 0;
                } else {
                    c += 1;
                }
            }
        }
    }
    let mut d = Doc::new(&text);
    if !cursors.is_empty() {
        d.cursors = cursors;
    }
    d
}
/// The text with the cursors marked again.
fn show(d: &Doc) -> String {
    let mut marks: Vec<(Pos, char)> = Vec::new();
    for k in &d.cursors {
        if k.has_selection() {
            marks.push((k.anchor(), '«'));
            marks.push((k.head(), '»'));
        } else {
            marks.push((k.head(), '|'));
        }
    }
    let mut out = String::new();
    for l in 0..d.line_count() {
        if l > 0 {
            out.push('\n');
        }
        for c in 0..=d.len(l) {
            out.extend(marks.iter().filter(|(p, _)| *p == pos(l, c)).map(|(_, m)| *m));
            if let Some(ch) = d.line(l).get(c) {
                out.push(*ch);
            }
        }
    }
    out
}

#[test]
fn line_endings_are_kept_and_positions_count_characters() {
    let mut d = Doc::new("a\r\nção 😀\r\n");
    assert_eq!((d.eol, d.line_count(), d.len(1)), ("\r\n", 3, 5));
    assert_eq!(d.text(), "a\r\nção 😀\r\n");
    d.cursors = vec![Cursor::at(pos(1, 5))];
    edit::backspace(&mut d, false, &style("plain"), 0);
    assert_eq!(d.text(), "a\r\nção \r\n");
}

#[test]
fn several_cursors_insert_delete_and_merge() {
    let mut d = doc("a|b\nc|d\n|");
    d.insert("xx", Kind::Type, 0);
    assert_eq!(show(&d), "axx|b\ncxx|d\nxx|");
    d.delete_by(Kind::Delete, 0, edit::left);
    assert_eq!(show(&d), "ax|b\ncx|d\nx|");
    // two cursors on one line stay in place after both insert
    let mut d = doc("|one |two");
    d.insert("> ", Kind::Type, 0);
    assert_eq!(show(&d), "> |one > |two");
    // overlapping deletions become one
    let mut d = doc("ab|c|d");
    d.delete_by(Kind::Delete, 0, |_, p| pos(p.l, 0));
    assert_eq!(show(&d), "|d");
}

#[test]
fn undo_groups_typing_and_returns_to_the_saved_state() {
    let mut d = Doc::new("");
    for (i, ch) in ["a", "b", "c"].iter().enumerate() {
        d.insert(ch, Kind::Type, i as i64 * 100);
    }
    d.mark_saved();
    // after a pause, a new step
    d.insert("d", Kind::Type, 5000);
    assert!(d.dirty());
    assert!(d.undo());
    assert_eq!(d.text(), "abc");
    assert!(!d.dirty(), "undo reaches the saved state");
    assert!(d.undo());
    assert_eq!(d.text(), "");
    assert!(!d.undo());
    assert!(d.redo());
    assert_eq!(d.text(), "abc");
    assert!(d.redo());
    assert_eq!(d.text(), "abcd");
    // a new change forgets the redo list
    d.undo();
    d.insert("z", Kind::Edit, 9000);
    assert!(!d.redo());
    // touch (the line ending) is a change without history
    d.mark_saved();
    d.touch();
    assert!(d.dirty());
}

#[test]
fn typing_pairs_brackets_and_quotes() {
    let s = style("js");
    let mut d = doc("f|");
    edit::type_text(&mut d, "(", &s, 0);
    assert_eq!(show(&d), "f(|)");
    edit::type_text(&mut d, ")", &s, 0);
    assert_eq!(show(&d), "f()|", "the closer is stepped over");
    // no pair before a word, nor a quote after one
    let mut d = doc("|x");
    edit::type_text(&mut d, "(", &s, 0);
    assert_eq!(show(&d), "(|x");
    let mut d = doc("don|");
    edit::type_text(&mut d, "'", &s, 0);
    assert_eq!(show(&d), "don'|");
    // a selection is wrapped and stays selected
    let mut d = doc("«word»");
    edit::type_text(&mut d, "\"", &s, 0);
    assert_eq!(show(&d), "\"«word»\"");
    // backspace inside an empty pair deletes both
    let mut d = doc("a[|]b");
    edit::backspace(&mut d, false, &s, 0);
    assert_eq!(show(&d), "a|b");
}

#[test]
fn new_lines_indent_and_open_blocks() {
    let s = style("js");
    let mut d = doc("  if (x) {|}");
    edit::newline(&mut d, &s, 0);
    assert_eq!(show(&d), "  if (x) {\n    |\n  }");
    let mut d = doc("  a = 1;|");
    edit::newline(&mut d, &s, 0);
    assert_eq!(show(&d), "  a = 1;\n  |");
    let mut d = doc("key:|");
    edit::newline(&mut d, &style("yaml"), 0);
    assert_eq!(show(&d), "key:\n  |");
    // plain text does not open blocks
    let mut d = doc("note:|");
    edit::newline(&mut d, &style("plain"), 0);
    assert_eq!(show(&d), "note:\n|");
}

#[test]
fn backspace_in_indentation_goes_to_the_tab_stop() {
    let s = style("js");
    let mut d = doc("     |x");
    edit::backspace(&mut d, false, &s, 0);
    assert_eq!(show(&d), "    |x");
    edit::backspace(&mut d, false, &s, 0);
    assert_eq!(show(&d), "  |x");
    let mut d = doc("one two  |");
    edit::backspace(&mut d, true, &s, 0);
    assert_eq!(show(&d), "one |");
}

#[test]
fn indent_comment_duplicate_and_move_lines() {
    let s = style("js");
    let mut d = doc("«a\nb\n»c");
    edit::indent_lines(&mut d, false, &s, 0);
    assert_eq!(d.text(), "  a\n  b\nc", "a selection ending at column 0 leaves that line");
    edit::indent_lines(&mut d, true, &s, 0);
    assert_eq!(d.text(), "a\nb\nc");

    let mut d = doc("«  a\n\n    b»");
    edit::toggle_comment(&mut d, &s, 0);
    assert_eq!(d.text(), "  // a\n\n  //   b");
    edit::toggle_comment(&mut d, &s, 0);
    assert_eq!(d.text(), "  a\n\n    b");

    let mut d = doc("  <p>x</p>|");
    edit::toggle_comment(&mut d, &style("html"), 0);
    assert_eq!(d.text(), "  <!-- <p>x</p> -->");
    edit::toggle_comment(&mut d, &style("html"), 0);
    assert_eq!(d.text(), "  <p>x</p>");

    let mut d = doc("a|\nb\nc");
    edit::duplicate_lines(&mut d, 0);
    assert_eq!(show(&d), "a\na|\nb\nc");
    edit::move_lines(&mut d, false, 0);
    assert_eq!(show(&d), "a\nb\na|\nc");
    edit::move_lines(&mut d, true, 0);
    assert_eq!(show(&d), "a\na|\nb\nc");
}

#[test]
fn next_occurrence_and_the_clipboard() {
    let mut d = doc("fo|o bar foo\nfoo");
    assert!(edit::select_next(&mut d).is_some());
    assert_eq!(show(&d), "«foo» bar foo\nfoo");
    edit::select_next(&mut d);
    edit::select_next(&mut d);
    assert_eq!(show(&d), "«foo» bar «foo»\n«foo»");
    assert_eq!(edit::select_next(&mut d), None, "every occurrence is selected");
    assert_eq!(edit::copy_text(&d), "foo\nfoo\nfoo");
    // as many lines as cursors: one for each
    edit::paste(&mut d, "1\r\n2\r\n3\r\n", 0);
    assert_eq!(show(&d), "1| bar 2|\n3|");
    let mut d = doc("one\nt|wo\nthree");
    assert_eq!(edit::copy_text(&d), "two\n");
    assert_eq!(edit::cut(&mut d, 0), "two\n");
    assert_eq!(show(&d), "one\n|three");
}

#[test]
fn movement_keeps_the_goal_column_and_collapses_selections() {
    let s = style("js");
    let mut d = doc("abcdef|\nab\nabcdef");
    edit::move_cursors(&mut d, edit::Motion::Down(1), false, &s);
    assert_eq!(show(&d), "abcdef\nab|\nabcdef");
    edit::move_cursors(&mut d, edit::Motion::Down(1), false, &s);
    assert_eq!(show(&d), "abcdef\nab\nabcdef|");
    edit::move_cursors(&mut d, edit::Motion::Up(5), false, &s);
    assert_eq!(show(&d), "|abcdef\nab\nabcdef");
    let mut d = doc("a«bcd»e");
    edit::move_cursors(&mut d, edit::Motion::Left, false, &s);
    assert_eq!(show(&d), "a|bcde");
    let mut d = doc("  x = y|");
    edit::move_cursors(&mut d, edit::Motion::LineHome, false, &s);
    assert_eq!(show(&d), "  |x = y");
    edit::move_cursors(&mut d, edit::Motion::LineHome, false, &s);
    assert_eq!(show(&d), "|  x = y");
    // a tab is as wide as the distance to the next stop; a wide character is measured
    assert_eq!(edit::column_at(&['\t', 'x'], 21., &s), 1);
    assert_eq!(edit::column_at(&['世', 'x'], 12., &s), 1);
    assert_eq!(edit::expand(&['a', '\t', 'b'], 4), "a   b");
}

#[test]
fn the_keyboard_map() {
    let s = style("js");
    let key = |name: &'static str, text: &'static str, ctrl: bool, shift: bool| edit::Key {
        name,
        text,
        base: text,
        shift,
        ctrl,
        alt: false,
        meta: false,
    };
    let mut d = doc("ab|");
    assert!(edit::key(&mut d, &key("c", "c", false, false), &s, 0).unwrap().changed);
    assert_eq!(d.text(), "abc");
    assert!(edit::key(&mut d, &key("z", "z", true, false), &s, 10).unwrap().changed);
    assert_eq!(d.text(), "ab");
    let copy = edit::key(&mut d, &key("c", "c", true, false), &s, 0).unwrap();
    assert_eq!(copy.copy.as_deref(), Some("ab\n"));
    assert!(edit::key(&mut d, &key("v", "v", true, false), &s, 0).unwrap().paste);
    let save = edit::key(&mut d, &key("s", "s", true, true), &s, 0).unwrap();
    assert_eq!(save.window, Some(("s".into(), true)));
    let escape = edit::key(&mut d, &key("Escape", "", false, false), &s, 0).unwrap();
    assert_eq!(escape.window, Some(("escape".into(), false)));
    assert_eq!(edit::key(&mut d, &key("F5", "", false, false), &s, 0), None);
    // Tab fills to the next stop
    let mut d = doc("a|");
    edit::key(&mut d, &key("Tab", "\t", false, false), &s, 0);
    assert_eq!(show(&d), "a |");
}

#[test]
fn languages_by_name_and_tokens_with_their_states() {
    for (name, id) in [("a/b/x.RS", "rust"), ("Makefile", "shell"), (".bashrc", "shell"), (".gitconfig", "ini"),
        ("page.tsx", "js"), ("noext", "plain"), ("a.unknown", "plain"), ("x.", "plain"), ("", "plain")] {
        assert_eq!(syntax::for_name(name).id, id, "{name}");
    }
    let run = |language: &str, line: &str, state: &str| {
        let (tokens, end) = syntax::tokenize(syntax::by_id(language), line, state);
        assert_eq!(tokens.iter().map(|t| t.0.as_str()).collect::<String>(), line, "tokens cover the line");
        (tokens.into_iter().filter(|t| !t.0.trim().is_empty()).map(|t| (t.0.trim().to_owned(), t.1)).collect::<Vec<_>>(), end)
    };
    let (tokens, end) = run("js", "const x = foo(\"a\"); /* open", "root");
    assert_eq!(end, "root/block");
    assert!(tokens.contains(&("const".into(), "keyword")) && tokens.contains(&("foo".into(), "function")));
    assert!(tokens.contains(&("\"a\"".into(), "string")));
    let (tokens, end) = run("js", "still */ done", &end);
    assert_eq!((tokens[0].1, end.as_str()), ("comment", "root"));
    // a regular expression is not a division, and a word inside a word is no keyword
    let (tokens, _) = run("js", "s.split(/\\s+/g); a / b; notconst", "root");
    assert!(tokens.contains(&("/\\s+/g".into(), "regexp")));
    assert!(!tokens.iter().any(|t| t.1 == "keyword"), "{tokens:?}");
    let (tokens, end) = run("python", "x = \"\"\"a \"quoted\" \\\"\"\" word", "root");
    assert_eq!(end, "root/tdq");
    assert_eq!(tokens.last().unwrap().1, "string");
    let (tokens, end) = run("python", "\"\"\" + 1", &end);
    assert_eq!((end.as_str(), tokens.last().unwrap().1), ("root", "number"));
    let (_, end) = run("html", "<!-- a - b", "root");
    assert_eq!(end, "root/comment");
    assert_eq!(run("html", "-->", &end).1, "root");
    assert!(run("rust", "'a' &'static str; println!(x)", "root").0.contains(&("println!".into(), "function")));
    assert!(run("sql", "SeLeCt id FROM t", "root").0.contains(&("SeLeCt".into(), "keyword")));
    assert_eq!(run("yaml", "key: - value", "root").0[0], ("key".into(), "attr"));
    for l in syntax::languages() {
        run(l.id, "x = \"é 世 😀\" # // /* <!-- ```", "root");
    }
}

#[test]
fn find_replace_and_the_file_score() {
    let d = Doc::new("ab AB ab\nç ab");
    let query = |text: &str, regex: bool, case_sensitive: bool| search::Query { text: text.into(), regex, case_sensitive };
    let found = search::find(&d, &query("ab", false, false));
    assert_eq!(found.list.len(), 4);
    assert_eq!((found.list[3].line, found.list[3].column), (1, 2), "columns count characters");
    assert_eq!(search::find(&d, &query("ab", false, true)).list.len(), 3);
    assert!(search::find(&d, &query("(", true, false)).bad_pattern);
    assert_eq!(search::find(&d, &query("(", false, false)).list.len(), 0);
    assert_eq!(search::find(&d, &query("x*", true, false)).list.len(), 0, "empty matches do not count");

    let found = search::find(&d, &query("(a)(?<second>b)", true, true));
    let edit = found.edit(&d, &found.list[0], "$2$1 $<second> $& $$ $9");
    assert_eq!(edit.text, "ba b ab $ $9");
    let literal = search::find(&d, &query("ab", false, false));
    assert_eq!(literal.edit(&d, &literal.list[0], "$1").text, "$1", "plain text is replaced as written");

    let mut d = Doc::new("x x x");
    let found = search::find(&d, &query("x", false, false));
    assert_eq!(found.next(&d).unwrap().column, 0);
    assert_eq!(found.previous(&d).unwrap().column, 4, "before the first one comes the last");
    d.cursors = vec![Cursor::range(pos(0, 2), pos(0, 3))];
    assert_eq!(found.current(&d), 1);
    assert_eq!(found.selected(&d).unwrap().column, 2);
    assert_eq!(found.next(&d).unwrap().column, 4);

    assert!(search::fuzzy("em", "editor/mod.rs") > search::fuzzy("em", "theme.rs"));
    assert_eq!(search::fuzzy("zz", "a/z.rs"), None);
}

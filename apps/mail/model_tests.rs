use super::html::{self, Picture};
use super::model::*;
use serde_json::{json, Value};
use std::collections::HashMap;

#[test]
fn attachment_names_become_plain_safe_base_names() {
    for (given, safe) in [
        ("report.pdf", "report.pdf"),
        ("../../etc/passwd", "passwd"),
        ("..\\..\\boot.ini", "boot.ini"),
        ("  ..hidden  ", "hidden"),
        ("a<b>c:d\"e|f?g*h", "a_b_c_d_e_f_g_h"),
        ("evil\u{202e}fdp.exe", "evil_fdp.exe"),
        ("trailing. . ", "trailing"),
        ("", "attachment"),
        ("...", "attachment"),
        ("a/", "attachment"),
        ("__", "attachment"),
        ("relatório ção.txt", "relatório ção.txt"),
    ] {
        assert_eq!(safe_name(given), safe, "{given:?}");
    }
    let long = safe_name(&"é".repeat(300));
    assert!(long.len() <= 200 && long.chars().all(|c| c == 'é'));
}

#[test]
fn sizes_and_document_glyphs() {
    assert_eq!(format_size(0.), "0 B");
    assert_eq!(format_size(1536.), "2 KB");
    assert_eq!(format_size(3. * 1_048_576.), "3.0 MB");
    assert_eq!(doc_glyph("a.PDF", "application/pdf"), ("PDF".into(), "#e5483f"));
    assert_eq!(doc_glyph("noext", "image/png").0, "");
}

#[test]
fn typed_markdown_never_becomes_raw_html() {
    let html = markdown_html("<script>alert(1)</script> **bold** [x](javascript:alert(1)) [ok](https://a.example/?q=\"1\")");
    assert!(!html.contains("<script") && !html.contains("javascript:"), "{html}");
    assert!(html.contains("&lt;script&gt;") && html.contains("<b>bold</b>"), "{html}");
    assert!(html.contains("href=\"https://a.example/?q=&quot;1&quot;\""), "{html}");
    assert_eq!(markdown_text("**hi** [site](https://x.example) [https://y](https://y)"), "*hi* site <https://x.example> https://y");
    let lists = markdown_html("- [x] done\n- [ ] open\n\n> quoted\n\n```\n<b>\n```");
    assert!(lists.contains("☑ done") && lists.contains("☐ open") && lists.contains("<blockquote") && lists.contains("&lt;b&gt;"), "{lists}");
}

#[test]
fn recipients_are_read_and_checked() {
    assert_eq!(parse_recipient(" \"Bo Li\" <bo@example.org> "), address("Bo Li", "bo@example.org"));
    assert_eq!(parse_recipient("bo@example.org"), address("", "bo@example.org"));
    assert!(email_ok("a.b+c@d.example"));
    for bad in ["", "a", "a@b", "a @b.com", "a@b.com,", "<a@b.com>", "a@@b.com", "a@.com"] {
        assert!(!email_ok(bad), "{bad:?}");
    }
}

fn words() -> DraftWords {
    DraftWords { quote_head: "Ann wrote:".into(), forwarded: "Forwarded".into(), from: "From".into(), date: "Date".into(), subject: "Subject".into(), to: "To".into(), when: "today".into() }
}

#[test]
fn replies_leave_me_out_and_forwards_carry_the_headers() {
    let message = json!({"acct":"a","folder":"INBOX","uid":4,"subject":"Plan","text":"one\n> two","msgid":"<2>","refs":"<1>",
        "from":[{"name":"Ann","email":"ann@x.org"}],"to":[{"name":"","email":"Me@x.org"},{"name":"Bo","email":"bo@x.org"}],"cc":[{"name":"","email":"ann@x.org"}]});
    let all = reply_draft(&message, Reply::All, "me@x.org", &words());
    assert_eq!(all["to"], json!([{"name":"Ann","email":"ann@x.org"}]));
    assert_eq!(all["cc"], json!([{"name":"Bo","email":"bo@x.org"}]));
    assert_eq!(all["subject"], "Re: Plan");
    assert_eq!(all["references"], "<1> <2>");
    assert_eq!(all["body"], "\n\nAnn wrote:\n> one\n>> two");
    assert_eq!(reply_draft(&message, Reply::Sender, "me@x.org", &words())["cc"], json!([]));
    let forward = reply_draft(&message, Reply::Forward, "me@x.org", &words());
    assert_eq!(forward["subject"], "Fwd: Plan");
    assert_eq!(forward["original"]["forward"], true);
    assert!(forward["body"].as_str().unwrap().contains("From: Ann <ann@x.org>\nDate: today\nSubject: Plan\nTo: Me@x.org, bo@x.org"));
    let again = json!({"subject":"RE: Plan","from":[{"name":"","email":"me@x.org"}]});
    let own = reply_draft(&again, Reply::Sender, "me@x.org", &words());
    assert_eq!(own["subject"], "RE: Plan");
    assert_eq!(own["to"], json!([{"name":"","email":"me@x.org"}]), "answering yourself still has a recipient");
}

#[test]
fn the_signature_goes_above_the_quote() {
    assert_eq!(initial_body("", ""), "");
    assert_eq!(initial_body("", "Me"), "\n\n-- \nMe");
    assert_eq!(initial_body("\n\n> q", "Me"), "\n\n-- \nMe\n\n> q");
}

#[test]
fn links_and_mailto() {
    assert_eq!(link_host("https://a.example/path?x#y"), "a.example");
    assert_eq!(link_host("a.example/path"), "a.example");
    assert_eq!(mailto_draft("MAILTO:a%40b.com").unwrap()["to"][0]["email"], "a@b.com");
    assert!(mailto_draft("https://a").is_none() && mailto_draft("ção").is_none());
}

#[test]
fn providers_ports_and_account_ids() {
    let gmail = autoconfig("A@GMail.com").unwrap();
    assert_eq!(gmail["known"], true);
    assert_eq!(gmail["davNote"], "oauth");
    let guess = autoconfig("a@corp.example").unwrap();
    assert_eq!(guess["imap"], json!({"host":"imap.corp.example","port":993,"security":"ssl"}));
    assert_eq!(guess["known"], false);
    assert!(autoconfig("nobody").is_none());
    assert_eq!([port_for(993, "starttls"), port_for(143, "ssl"), port_for(587, "ssl"), port_for(465, "starttls"), port_for(2525, "ssl")], [143, 993, 465, 587, 2525]);
    assert_eq!(account_id("João@Example.com", &[]), "jo-o-example-com");
    assert_eq!(account_id("a", &["a", "a-2"]), "a-3");
    assert_eq!(account_id("***", &[]), "account");
}

#[test]
fn the_list_asks_for_the_rows_in_view_and_actions_stay_in_one_folder() {
    let (op, args) = Source::Unified { filter: "unread", label: 0 }.request(10, 20);
    assert_eq!((op, args), ("list", json!({"folder":"*inbox","threaded":true,"offset":10,"count":20,"filter":"unread"})));
    assert_eq!(Source::Unified { filter: "all", label: 3 }.request(0, 5).1["label"], 3);
    assert_eq!(Source::Folder { account: "a".into(), folder: "F".into() }.request(0, 5).1["folder"], "F");
    let thread = json!({"acct":"a","folder":"INBOX","uid":1,"thread":[{"acct":"a","folder":"INBOX","uid":1},{"acct":"a","folder":"Sent","uid":9},{"acct":"a","folder":"INBOX","uid":2}]});
    assert_eq!(targets(&thread), json!({"acct":"a","folder":"INBOX","uids":[1,2]}));
    assert!(is_unread(&json!({"unreadN":1,"seen":true})) && !is_unread(&json!({"seen":true})));
}

#[test]
fn list_dates_get_shorter_the_older_they_are() {
    let day = |year, yday| {
        let mut t: libc::tm = unsafe { std::mem::zeroed() };
        t.tm_year = year;
        t.tm_yday = yday;
        t
    };
    let (now, yesterday) = (day(125, 100), day(125, 99));
    let age = |days| days * 86_400_000_i64;
    assert_eq!(list_date(&day(125, 100), &now, &yesterday, 0), ListDate::Time);
    assert_eq!(list_date(&day(125, 99), &now, &yesterday, age(1)), ListDate::Yesterday);
    assert_eq!(list_date(&day(125, 97), &now, &yesterday, age(3)), ListDate::Weekday);
    assert_eq!(list_date(&day(125, 10), &now, &yesterday, age(90)), ListDate::Short);
    assert_eq!(list_date(&day(124, 300), &now, &yesterday, age(165)), ListDate::Numeric);
}

#[test]
fn saving_an_attachment_never_overwrites_or_leaves_downloads() {
    let root = std::env::temp_dir().join(format!("axia-mail-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (cache, home) = (root.join("cache"), root.join("home"));
    std::fs::create_dir_all(cache.join("att")).unwrap();
    std::fs::create_dir_all(home.join("Downloads")).unwrap();
    std::fs::write(cache.join("att/f"), b"data").unwrap();
    let first = save_to_downloads(&cache, "att/f", "../report.pdf", &home).unwrap();
    let second = save_to_downloads(&cache, "att/f", "report.pdf", &home).unwrap();
    assert_eq!(first, home.join("Downloads/report.pdf"));
    assert_eq!(second, home.join("Downloads/report (2).pdf"));
    assert_eq!(std::fs::read(&second).unwrap(), b"data");
    for escape in ["../secret", "/etc/passwd", "att/../../x", ""] {
        assert!(!inside_cache(escape), "{escape}");
        assert!(save_to_downloads(&cache, escape, "x", &home).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}

// ---- the reading pane ----

fn colors() -> html::Colors<'static> {
    html::Colors { text: "#eee", dim: "#999", accent: "#0af", dark: true }
}
fn walk<'a>(node: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(node);
    for child in node["children"].as_array().into_iter().flatten() {
        walk(child, out);
    }
}

#[test]
fn html_links_name_their_message_and_remote_images_wait() {
    let doc = json!({"blocks":[
        {"t":"p","c":[{"t":"x","x":"see here ","l":0},{"t":"img","src":"r:1"},{"t":"img","src":"cid:2","w":40}]},
        {"t":"tbl","s":{"w":400},"c":[{"t":"tr","c":[{"t":"td","s":{"w":100},"c":[]},{"t":"td","c":[{"t":"x","x":"b"}]}]}]}],
        "links":[{"h":"https://a.example","w":0}]});
    let none = HashMap::new();
    let (tree, wanted) = html::document("m|k", &doc, 600., false, &none, &colors());
    assert_eq!(wanted.iter().map(|w| w.source.as_str()).collect::<Vec<_>>(), ["cid:2"]);
    let mut nodes = Vec::new();
    walk(&tree, &mut nodes);
    let clicks: Vec<&Value> = nodes.iter().map(|n| &n["events"]["onClick"]).filter(|v| !v.is_null()).collect();
    assert!(!clicks.is_empty() && clicks.iter().all(|c| **c == json!(["link", ["m|k", 0]])), "{clicks:?}");
    assert!(nodes.iter().any(|n| n["style"]["width"] == 300.0), "the free cell takes what the fixed one leaves");
    let (_, wanted) = html::document("m|k", &doc, 600., true, &none, &colors());
    assert_eq!(wanted.len(), 2);
    let ready = HashMap::from([("r:1".to_owned(), Picture::Ready { handle: 7, width: 2000., height: 1000. })]);
    let (tree, _) = html::document("m|k", &doc, 600., true, &ready, &colors());
    let mut nodes = Vec::new();
    walk(&tree, &mut nodes);
    let image = nodes.iter().find(|n| n["kind"] == "mailImage").unwrap();
    assert!(image["style"]["width"].as_f64().unwrap() <= 572.);
}

#[test]
fn hostile_documents_stay_bounded() {
    let mut node = json!({"t":"x","x":"deep"});
    for _ in 0..500 {
        node = json!({"t":"div","c":[node]});
    }
    let doc = json!({"blocks":[node, {"t":"x","x":"word ".repeat(20_000),"l":99},{"t":"x","x":"again ".repeat(10)}, {"t":"img","src":"cid:x","w":1e12,"h":-4}]});
    let (tree, _) = html::document("k", &doc, 0., true, &HashMap::new(), &colors());
    let mut nodes = Vec::new();
    walk(&tree, &mut nodes);
    assert!(nodes.len() < 60_000, "{}", nodes.len());
}

#[test]
fn plain_text_links_and_quotes() {
    assert_eq!(html::split_links("go to https://a.example/x. now"), [("go to ", false), ("https://a.example/x", true), (". now", false)]);
    let tree = html::plain("p", "> quoted\nmailto:a@b.com", 500., &colors());
    assert_eq!(tree["children"][0]["style"]["color"], "#999");
    let mut nodes = Vec::new();
    walk(&tree, &mut nodes);
    assert!(nodes.iter().any(|n| n["events"]["onClick"] == json!(["url", "mailto:a@b.com"])));
}

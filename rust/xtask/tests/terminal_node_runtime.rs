//! A native Rust tree showing real terminal sessions: each `terminal` node is bound to the
//! compositor's renderer, paints the session it names (the profile colors of that session),
//! follows it when the app changes it and is released with the view. The harness user has
//! no login shell, so the sessions are told apart by their own background colors.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
type Rgb = (u8, u8, u8);
const FIRST: Rgb = (0x20, 0x70, 0x30);
const SECOND: Rgb = (0x70, 0x20, 0x60);
type Counts = (usize, usize, usize, usize);
fn counts(path: &std::path::Path, width: u32) -> Counts {
    let capture = image::open(path).unwrap().to_rgba8();
    assert_eq!((capture.width(), capture.height()), (width, 700));
    let near = |p: &image::Rgba<u8>, c: Rgb| p[0].abs_diff(c.0) < 6 && p[1].abs_diff(c.1) < 6 && p[2].abs_diff(c.2) < 6;
    let count = |c: Rgb, x0: u32, x1: u32| {
        (x0..x1).flat_map(|x| (0..640).map(move |y| (x, y))).filter(|(x, y)| near(capture.get_pixel(*x, *y), c)).count()
    };
    let half = width / 2;
    (count(FIRST, 0, half), count(SECOND, 0, half), count(FIRST, half, width), count(SECOND, half, width))
}
#[test]
#[ignore = "requires built compositor and npm ci; required by GPU=1 desktop CI"]
fn native_trees_show_real_terminal_sessions_and_rebind_them() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [760, 1000] {
            let shots = std::cell::RefCell::new(Vec::new());
            let report = ui::run_ui_with_capture_timeout(
                repo,
                "terminal_node",
                &DRIVER.replace("@MODE@", mode).replace("@WIDTH@", &width.to_string()),
                mode,
                width,
                "terminal.png",
                30,
                |path| {
                    shots.borrow_mut().push(counts(path, width));
                    let second = std::path::PathBuf::from(format!("{}.two.png", path.display()));
                    if second.exists() {
                        shots.borrow_mut().push(counts(&second, width));
                    }
                },
            );
            for flag in ["bound", "painted", "rebound", "second", "released"] {
                assert_eq!(report[flag], true, "{mode}/{width}/{flag}: {report}");
            }
            let shots = shots.into_inner();
            assert_eq!(shots.len(), 2, "{mode}/{width}: both pictures were taken: {report}");
            let pane = (width / 2) as usize * 640;
            // one pane: the first session fills the tree
            let (a, b, c, d) = shots[0];
            assert!(
                a > pane * 9 / 10 && c > pane * 9 / 10 && b == 0 && d == 0,
                "{mode}/{width}: the first session fills the tree alone ({a} / {c}, other {b} / {d}): {report}"
            );
            // two panes: the first session on the left, the second on the right
            let (left_first, left_second, right_first, right_second) = shots[1];
            assert!(
                left_first > pane * 9 / 10 && right_second > pane * 9 / 10,
                "{mode}/{width}: each pane paints its own session ({left_first} / {right_second}): {report}"
            );
            assert!(left_second == 0 && right_first == 0, "{mode}/{width}: no pane paints the other session ({left_second} / {right_first})");
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h} from 'react';
import React from 'react';
import {render,View} from '@lordos/runtime';
import {NativeView} from '@ROOT@/sdk/nativeview.js';
import {NATIVE_TREE,treeOwners} from '@ROOT@/sdk/nativetree.js';
import {media} from '@ROOT@/sdk/media.js';
const kind='terminal_node',sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let stage='bootstrap',tree=null,redraw=()=>{};
const owner='terminal-node-test';
const pane=(key,id)=>({kind:'terminal',key,session:id,style:{flex:1,height:640}});
const build=(first,second)=>({kind:'view',key:'root',style:{flex:1,flexDirection:'row',backgroundColor:'#101010'},children:[
  pane('left',first),...(second?[pane('right',second)]:[])]});
function Harness(){const [,set]=React.useState(0);redraw=()=>set(n=>n+1);
  return h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'#101010'}},h(NativeView,{manifest:{...NATIVE_TREE,shell:false},args:{owner,tree},style:{width:@WIDTH@,height:700}}));}
treeOwners.set(owner,()=>{});
render(h(Harness));
async function capture(suffix=''){await sleep(250);const image=lord.media.screenshot('screenshots');const result=await media.saveImage(image,@IMAGE@+suffix);media.close(image);return!!result?.ok;}
const info=id=>{try{return lord.term.info(id);}catch(_){return null;}};
async function run(){
 stage='create';
 const first=lord.term.create({cwd:lord.fs.home(),scrollback:50});
 const second=lord.term.create({cwd:lord.fs.home(),scrollback:50});
 lord.term.configure(first,{background:0x207030,foreground:0xffffff});
 lord.term.configure(second,{background:0x702060,foreground:0xffffff});
 tree=build(first,0);redraw();await sleep(500);
 const bound=!!info(first);
 stage='painted';
 const painted=await capture();
 stage='two panes';
 tree=build(first,second);redraw();await sleep(500);
 const second_ok=!!info(first)&&!!info(second);
 const saved=await capture('.two.png');
 stage='rebind';
 tree=build(second,first);redraw();await sleep(400);
 const rebound=!!info(first)&&!!info(second);
 stage='release';
 tree=null;lord.term.close(first);lord.term.close(second);
 await sleep(200);
 const released=!info(first)&&!info(second);
 console.log(kind+'-observation '+JSON.stringify({bound,painted:painted&&saved,rebound,second:second_ok,released}));
}
setTimeout(()=>run().catch(async error=>{await capture();console.log(kind+'-observation '+JSON.stringify({saved:true,error:String(error),stage}));}),200);
"#;

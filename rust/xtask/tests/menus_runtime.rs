#![cfg(target_os = "linux")]
//! Context menu integration against a real private compositor.
#[path = "support/ui.rs"]
mod ui;
use serde_json::json;
use std::path::Path;

#[test]
#[ignore = "requires built axia-server/axia-media and npm ci; required by GPU=1 desktop CI"]
fn native_context_menus_ignore_legacy_settings_and_preserve_interaction() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui(repo, "menus", DRIVER, mode, width, "menu.png");
            assert_eq!(report["ran"], json!(["alpha", "child"]), "{report}");
            assert_eq!(report["disabledRan"], false);
            assert_eq!(report["escaped"], true);
            assert_eq!(report["outsideClosed"], true);
            assert_eq!(report["saved"], true, "{report}");
        }
    }
}

const DRIVER: &str = r#"
import { createElement as h, useState, useRef } from 'react';
import { render, View } from '@lordos/runtime';
import { Menu } from '@ROOT@/sdk/menu.jsx';
import { ThemeProvider } from '@ROOT@/sdk/theme.js';
import { makeTheme } from '@ROOT@/themes/default/tokens.js';
import { settings } from '@ROOT@/sdk/settings.js';
import { enableShellLayers } from '@ROOT@/sdk/layers.js';
import { media } from '@ROOT@/sdk/media.js';
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const find=label=>{for(let handle=1;handle<256;handle++){const node=lord.nativeViews.findText(handle,label);if(node)return node;}return 0;};
let show, outside, closed=0, disabledRan=false, ran=[];
const theme=makeTheme({mode:'@MODE@',accent:'azul',effects:false});
function App(){const[visible,set]=useState(true);show=set;const ref=useRef();
 return h(ThemeProvider,{tokens:theme},h(View,{ref,style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='light'?'#eeeeee':'#182036'},onMouseMove:()=>{}},
   h(View,{ref:n=>{outside=n;},style:{width:30,height:30}}),visible&&h(Menu,{x:40,y:60,onClose:()=>{closed++;set(false);},items:[
    {label:'Alpha',onClick:()=>ran.push('alpha')},'-',{label:'Disabled',disabled:true,onClick:()=>{disabledRan=true;}},
    {label:'Branch',submenu:[{label:'Child',onClick:()=>ran.push('child')}]}]})));}
const pump=()=>globalThis.__lordNativeView();
const key=k=>{lord.nativeViews.key(k,'',0);pump();};
const press=node=>{const[x,y,w,hh]=lord.getLayout(node);for(const type of ['down','up']){const mount=lord.nativeViews.pointer(type,node,x+w/2,y+hh/2,0);__lordEvent(type,mount,x+w/2,y+hh/2,0);}pump();};
async function open(){show(true);for(let i=0;i<100&&!find('Alpha');i++)await sleep(20);}
async function run(){await open();press(find('Disabled'));await sleep(50);press(find('Alpha'));await sleep(60);
 await open();key('ArrowDown');key('ArrowDown');key('ArrowRight');await sleep(100);key('ArrowDown');key('Enter');await sleep(60);
 await open();let before=closed;key('Escape');await sleep(60);const escaped=closed>before;
 await open();before=closed;press(outside.id);await sleep(60);const outsideClosed=closed>before;
 await open();await sleep(200);const handle=lord.media.screenshot('screenshots');const saved=await media.saveImage(handle,@IMAGE@);media.close(handle);
 console.log('menus-observation '+JSON.stringify({ran,disabledRan,escaped,outsideClosed,saved:!!saved?.ok}));}
settings.set({language:'en',menuNativeView:false,nativeAppearance:'axia'});enableShellLayers();render(h(App));
setTimeout(()=>run().catch(e=>console.error('menus driver: '+(e.stack||e))),200);
"#;

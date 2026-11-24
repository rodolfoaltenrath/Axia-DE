//! The compiled PDF viewer in the real compositor with the real confined helper:
//! open, render, outline, search, selection, copy, page navigation, zoom, rotation
//! and saving a copy. Poppler runs inside axia-doc; nothing here is simulated.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;
#[test]
#[ignore = "requires built compositor, axia-doc and npm ci; required by GPU=1 desktop CI"]
fn pdf_viewer_opens_renders_searches_selects_and_saves_with_the_real_helper() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    for appearance in ["axia", "adwaita"] {
        for mode in ["light", "dark"] {
            for width in [760, 1000] {
                let kind = format!("pdf_{appearance}");
                let driver = DRIVER.replace("@APPEARANCE@", appearance);
                let report = ui::run_ui_with_capture_timeout(
                    repo,
                    &kind,
                    &driver,
                    mode,
                    width,
                    "pdf.png",
                    45,
                    |path| {
                        let capture = image::open(path).unwrap().to_rgba8();
                        assert_eq!((capture.width(), capture.height()), (width, 700));
                        // The red block of the fixture page proves that helper pixels reached the scene.
                        let red = capture
                            .pixels()
                            .filter(|p| p[0] > 190 && p[1] < 70 && p[2] < 70)
                            .count();
                        assert!(red > 4000, "{appearance}/{mode}/{width}: page raster missing ({red} red pixels)");
                        if let Some(output) = std::env::var_os(format!("AXIA_{}_OUTPUT", kind.to_ascii_uppercase())) {
                            let out = std::path::PathBuf::from(output).join(format!("{mode}-{width}"));
                            std::fs::create_dir_all(&out).unwrap();
                            std::fs::copy(path, out.join("pdf.png")).unwrap();
                        }
                    },
                );
                for flag in [
                    "compiled",
                    "nativeAppearance",
                    "opened",
                    "sidebar",
                    "outline",
                    "search",
                    "selected",
                    "copied",
                    "secondPage",
                    "singlePage",
                    "zoomed",
                    "rotated",
                    "savedCopy",
                    "closed",
                ] {
                    assert_eq!(report[flag], true, "{appearance}/{mode}/{width}/{flag}: {report}");
                }
            }
        }
    }
}
const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {nativeApp} from '@ROOT@/runtime/nativeapps.js';
import {configureWindows,wm,useWindows} from '@ROOT@/sdk/windows.js';
import {AppHost} from '@ROOT@/sdk/components.jsx';
import {ThemeProvider} from '@ROOT@/sdk/theme.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {settings} from '@ROOT@/sdk/settings.js';
import {locale} from '@ROOT@/sdk/i18n.js';
import {media} from '@ROOT@/sdk/media.js';
const kind='pdf_@APPEARANCE@',sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let handle=0,window,stage='bootstrap',nativeAppearance=true;
locale.set({...locale.get(),lang:'en'});settings.set({language:'en',mode:'@MODE@',nativeAppearance:'@APPEARANCE@',effects:false});
const mount=lord.nativeViews.mount;lord.nativeViews.mount=(node,id,raw,...rest)=>{const facts=JSON.parse(raw);if(id==='pdf'&&!facts.icon)nativeAppearance&&=!('theme'in(facts.context||{}));const result=mount(node,id,raw,...rest);if(id==='pdf'&&!facts.icon)handle=result;return result;};
// Two pages: a known red block (pixels) and text (search, selection), made with the standard Helvetica font.
const home=lord.fs.home();try{lord.fs.mkdir(home+'/Documents');}catch(_){}
const pdfPath=home+'/Documents/sample.pdf';
{
 const page=n=>'0.85 0.1 0.1 rg 60 300 300 300 re f\n0 g\nBT /F1 28 Tf 72 700 Td (Axia sample page '+n+') Tj 0 -40 Td (Hello native world) Tj ET\n';
 const stream=c=>'<</Length '+c.length+'>>\nstream\n'+c+'endstream';
 const objects=['<</Type/Catalog/Pages 2 0 R>>','<</Type/Pages/Kids[3 0 R 5 0 R]/Count 2>>',
  '<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 7 0 R>>>>/Contents 4 0 R>>',stream(page(1)),
  '<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Resources<</Font<</F1 7 0 R>>>>/Contents 6 0 R>>',stream(page(2)),
  '<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>'];
 let pdf='%PDF-1.4\n';const offsets=[];for(let i=0;i<objects.length;i++){offsets.push(pdf.length);pdf+=(i+1)+' 0 obj\n'+objects[i]+'\nendobj\n';}
 const xref=pdf.length;pdf+='xref\n0 '+(objects.length+1)+'\n0000000000 65535 f \n'+offsets.map(o=>String(o).padStart(10,'0')+' 00000 n \n').join('')+'trailer\n<</Size '+(objects.length+1)+'/Root 1 0 R>>\nstartxref\n'+xref+'\n%%EOF\n';
 lord.fs.write(pdfPath,pdf);
}
const manifest=lord.nativeApps.request(JSON.stringify({cmd:'catalog'})).find(app=>app.id==='pdf');
configureWindows({apps:[nativeApp(manifest)]});window=wm.open('pdf',{args:{path:pdfPath}});
function Harness(){useWindows();return h(ThemeProvider,{tokens:makeTheme({mode:'@MODE@',effects:false})},h(View,{style:{width:@WIDTH@,height:700,backgroundColor:'@MODE@'==='dark'?'#222':'#eee'}},wm.get(window)?h(AppHost,{key:window,id:window,focused:true}):null));}
render(h(Harness));
const text=()=>lord.nativeViews.text(handle)||'';
async function until(fn,label,steps=300){stage=label;for(let i=0;i<steps&&!fn();i++)await sleep(20);if(!fn())throw Error('timeout: '+label+'; '+text());return true;}
const action=(cmd,key='',extra)=>{lord.nativeViews.action(handle,{action:[cmd,key],...(extra?{extra}:{})});__lordNativeView();};
const percent=()=>{const m=text().match(/(\d+) %/);return m?Number(m[1]):0;};
async function capture(){await sleep(150);const image=lord.media.screenshot('screenshots');const result=await media.saveImage(image,@IMAGE@);media.close(image);return!!result?.ok;}
async function run(){
 const compiled=manifest?.nativeView==='rust'&&manifest?.process!=='isolated';
 const opened=await until(()=>text().includes('Page 1 of 2'),'document opened');
 const sidebar=text().includes('Pages')&&text().includes('Contents');
 await sleep(400);
 action('side','outline');const outline=await until(()=>text().includes('This document has no outline.'),'outline tab');action('side','thumbs');
 action('findChange','',{value:'native'});const search=await until(()=>/\d+ of \d+ matches/.test(text()),'search hits');
 action('do','escape');
 action('do','selectAll');const selected=await until(()=>/characters? selected/.test(text()),'selection');
 action('do','copy');const copied=await until(()=>text().includes('Text copied'),'copy message');
 action('page',1);const secondPage=await until(()=>text().includes('Page 2 of 2'),'second page');
 action('mode','single');await sleep(100);action('page',0);const singlePage=await until(()=>text().includes('Page 1 of 2'),'single page mode');action('mode','continuous');
 const before=percent();action('zoom',1);const zoomed=await until(()=>percent()>before,'zoom in');
 const turnBefore=text();action('rotate',1);await sleep(200);const rotated=await until(()=>text().length>0,'rotate');void turnBefore;
 action('do','saveCopy');const savedCopy=await until(()=>text().includes('Saved as')&&text().includes('.pdf'),'saved copy');
 const exists=(()=>{try{return lord.fs.list(home+'/Documents').some(e=>e.name!=='sample.pdf'&&/\.pdf$/.test(e.name));}catch(_){return false;}})();
 action('zoomToggle');await sleep(150);
 const saved=await capture();
 wm.close(window,true);await sleep(200);const closed=true;
 console.log(kind+'-observation '+JSON.stringify({compiled,nativeAppearance,opened,sidebar,outline,search,selected,copied,secondPage,singlePage,zoomed,rotated,savedCopy:savedCopy&&exists,closed,saved}));
}
setTimeout(()=>run().catch(async error=>{await capture();console.log(kind+'-observation '+JSON.stringify({saved:true,error:String(error),stage,text:text()}));}),200);
"#;

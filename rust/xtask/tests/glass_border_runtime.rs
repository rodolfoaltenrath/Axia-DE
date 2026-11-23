//! Dock glass must not introduce colored fringes over a neutral backdrop.
#![cfg(target_os = "linux")]
#[path = "support/ui.rs"]
mod ui;

#[test]
#[ignore = "requires built axia-server and npm ci; required by GPU=1 desktop CI"]
fn dock_glass_edges_keep_the_material_color() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors().nth(2).unwrap();
    for mode in ["light", "dark"] {
        for width in [640, 1000] {
            let report = ui::run_ui_with_capture(repo, "glass_border", DRIVER, mode, width,
                "glass-border.png", |path| {
                    let pixels = image::open(path).unwrap().into_rgb8();
                    let background = if mode == "dark" { 34 } else { 238 };
                    let mut changed = 0;
                    // All four panels are neutral, including fractional rounded edges.
                    // The material itself has a small blue tint (up to ten levels).
                    for (x, y, pixel) in pixels.enumerate_pixels() {
                        let [r, g, b] = pixel.0;
                        assert!(r.abs_diff(g) <= 1 && b.abs_diff(r) <= 12,
                            "colored glass fringe {mode}/{width} at {x},{y}: {:?}", pixel.0);
                        changed += usize::from(pixel.0 != [background; 3]);
                    }
                    assert!(changed > 20000, "glass panels must actually be rendered");
                });
            assert_eq!(report["saved"], true);
        }
    }
}

const DRIVER: &str = r#"
import {createElement as h} from 'react';
import {render,View} from '@lordos/runtime';
import {dockGlass} from '@ROOT@/sdk/glass.js';
import {makeTheme} from '@ROOT@/themes/default/tokens.js';
import {media} from '@ROOT@/sdk/media.js';
const mode='@MODE@';
const panels=[true,false].flatMap((effects,row)=>[0,0.25].map((fraction,column)=>
 h(View,{style:{...dockGlass(makeTheme({mode,effects})),position:'absolute',
 left:40+column*280+fraction,top:100+row*240+fraction,width:240.5,height:167.5,
 borderRadius:20}})));
render(h(View,{style:{width:@WIDTH@,height:700,backgroundColor:mode==='dark'?'#222':'#eee'}},...panels));
setTimeout(async()=>{try{
 const image=lord.media.screenshot('screenshots'),saved=await media.saveImage(image,@IMAGE@);media.close(image);
 console.log('glass_border-observation '+JSON.stringify({saved:!!saved?.ok}));
}catch(error){console.error('glass_border driver: '+(error.stack||error));}},500);
"#;

// Generic transport for a Rust session that publishes native desktop mounts.
// Rust supplies the widget list, geometry, pin state and all mounted contents.
import { createElement as h } from 'react';
import { View, screen } from '@lordos/runtime';
import { registerWidgetType, setWidgets } from '../sdk/widgets.js';
import * as fs from '../sdk/fs.js';
const sessions = new Map();
export function startNativeSession(manifest, Mount) {
  if (!manifest.sessionWidgets || sessions.has(manifest.id)) return;
  const model=lord.nativeApps.create(manifest.id);
  const nativeManifest={...manifest,shell:true};
  registerWidgetType(manifest.sessionWidgets,function NativeDesktopMount({widget}) {
    return h(View,{style:widget.rect},h(Mount,{manifest:nativeManifest,args:{widget:widget.id,rect:widget.rect}}));
  });
  let revision=0,geometry='';
  const poll=()=>{
    const size={width:screen.width,height:screen.height};
    const changed=geometry!==JSON.stringify(size);geometry=JSON.stringify(size);
    const result=model.request(JSON.stringify({cmd:'background',home:fs.home(),screen:size,revision:changed?0:revision}));
    if(!result.same&&result.widgets){revision=result.revision;setWidgets(manifest.sessionWidgets,result.widgets);}
  };
  sessions.set(manifest.id,{model,timer:setInterval(poll,200)});
  queueMicrotask(poll);
}

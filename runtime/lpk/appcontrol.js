// Local-window compatibility for isolated apps. The session excludes this module.
import {windows,wm,getApp,onActiveDesktop,activeDesktop} from '../../sdk/windows.js';
import {launcherIdOf,dockIdOfWindow,quitApp} from '../../sdk/dock.js';
import {linuxAppFor,shortAppName,windowAppName} from '../../sdk/apps.js';
import {t,appName} from '../../sdk/i18n.js';
const isPanel=w=>!!getApp(w.id)?.builtin&&!String(w.id).startsWith('linux#');
const hiddenByDesktop=/*@__PURE__*/new Map();
function groupName(key,w){const app=getApp(key);if(app)return appName(app);
 const linux=linuxAppFor(String(key).replace(/^desktop:/,''));return linux?shortAppName(linux):w?windowAppName(w):String(key);}
export function appGroup(id){const w=wm.get(id);if(!w)return null;const key=launcherIdOf(w);
 return{key,name:groupName(key,w),windows:windows.get().filter(w=>launcherIdOf(w)===key&&!isPanel(w))};}
export function hideApp(id=wm.focused()){appGroup(id)?.windows.forEach(w=>wm.minimize(w.id));}
export function hideOthers(id=wm.focused()){const group=appGroup(id);if(!group)return;const mine=new Set(group.windows.map(w=>w.id));
 windows.get().forEach(w=>{if(!mine.has(w.id)&&!isPanel(w)&&onActiveDesktop(w))wm.minimize(w.id);});}
export function showAllApps(){windows.get().forEach(w=>{if(w.minimized)wm.raise(w.id);});}
export function toggleShowDesktop(){const desk=activeDesktop.get(),visible=windows.get().filter(w=>!w.minimized&&!w.pip&&!isPanel(w)&&onActiveDesktop(w));
 if(visible.length){hiddenByDesktop.set(desk,visible.map(w=>w.id));visible.forEach(w=>wm.minimize(w.id));return;}
 (hiddenByDesktop.get(desk)||[]).forEach(id=>{if(wm.get(id)?.desktop===desk)wm.raise(id);});hiddenByDesktop.delete(desk);}
export function cycleAppWindows(back){const top=wm.get(wm.focused());if(!top)return;const key=dockIdOfWindow(top);
 const mine=windows.get().filter(w=>dockIdOfWindow(w)===key&&!w.minimized&&onActiveDesktop(w));if(mine.length<2)return;
 if(!back){wm.focus(mine[0].id);return;}wm.toBack(top.id);wm.focus(mine[mine.length-2].id);}
export function quitAppGroup(identity={}){const top=wm.get(identity.id||wm.focused()),key=identity.key||(top&&launcherIdOf(top));
 if(key)quitApp(windows.get().filter(w=>launcherIdOf(w)===key));}
export function appMenuItems(id,settingsItemOf){const group=appGroup(id);if(!group)return[];const settingsItem=settingsItemOf(id);
 return[{label:t('shell.app.about',{app:group.name}),onClick:()=>wm.open('appabout',{new:true,args:{key:group.key,win:id}})},'-',
 {label:t('shell.app.settings'),hint:'❖,',disabled:!settingsItem,onClick:()=>settingsItem?.onClick()},'-',
 {label:t('shell.app.hide',{app:group.name}),hint:'❖H',onClick:()=>hideApp(id)},
 {label:t('shell.app.hideOthers'),hint:'❖⌥H',onClick:()=>hideOthers(id)},
 {label:t('shell.app.showAll'),onClick:showAllApps},'-',
 {label:t('shell.app.quit',{app:group.name}),hint:'❖Q',onClick:()=>quitApp(group.windows)}];}

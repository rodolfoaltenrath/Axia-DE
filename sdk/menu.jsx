// The session always draws menus with Rust. The isolated JavaScript app host
// keeps its existing React menu, selected at build time rather than by a setting.
import { NativeMenu } from './nativemenu.jsx';
import { AppMenu } from '../runtime/lpk/menu.jsx';

export function Menu(props) {
  if (typeof __AXIA_APP_HOST__ !== 'undefined' && __AXIA_APP_HOST__) return <AppMenu {...props} />;
  return <NativeMenu {...props} />;
}

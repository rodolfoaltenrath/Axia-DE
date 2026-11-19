// Shared app metadata for native views and the session app-control service.
import { appRegistry, appCatalogSource } from './windows.js';
import { launcherCatalog, appIconFacts, shortAppName } from './apps.js';
import { locale, appName } from './i18n.js';
import { createFactsStore } from './nativeviewfacts.js';

export const appFacts = app => ({ id: app.id, name: app.name, kind: app.kind, version: app.version,
  desktopId: app.desktopId, wmClass: app.wmClass, mimeTypes: app.mimeTypes,
  builtin: !!app.builtin, hidden: !!app.hidden, live: !!app.live, shortName: app.kind === 'linux' ? shortAppName(app) : undefined,
  comment: app.comment, description: app.description, genericName: app.genericName,
  iconPath: typeof app.icon === 'string' ? app.icon : '',
  package: !!app.package, system: !!app.system, isolated: !!app.isolated, icon: !!app.Icon,
  iconData: appIconFacts(app),
  category: typeof app.category === 'string' ? app.category : undefined });

// Only the metadata actually read by app control crosses its bounded bridge.
function controlFacts(app) {
  const { id, name, kind, desktopId, wmClass, builtin, hidden, live, shortName, package: packaged, isolated, icon, iconData } = appFacts(app);
  return { id, name, kind, desktopId, wmClass, builtin, hidden, live, shortName, package: packaged, isolated, icon, iconData };
}
let visible, control;
export function nativeCatalogFacts(appControl = false) {
  const current = appControl ? control : visible;
  if (current) return current;
  const encode = appControl ? controlFacts : appFacts;
  const store = createFactsStore([launcherCatalog, appRegistry, locale],
    () => [launcherCatalog.get(), appRegistry.get(), locale.get()],
    () => ({
      // Identity matching keeps source order, as lord.launcher does. The
      // visible catalog is name-sorted and can resolve an ambiguous class differently.
      catalog: (appControl ? [...launcherCatalog.get().byId.values()] : launcherCatalog.get().apps).map(encode),
      registry: appCatalogSource().map(app => encode({ ...app, name: appName(app) })),
    }));
  if (appControl) control = store; else visible = store;
  return store;
}

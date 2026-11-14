// Shared settings transport; native applications construct their own patches.
export function settingsServices(settings) {
  return { 'settings.patch': { run([patch]) { settings.set(patch); return settings.get(); } } };
}

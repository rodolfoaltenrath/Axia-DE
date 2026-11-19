// Version of the JavaScript module contract used by installed .lpk apps.
// Independent of the desktop/app versions, package signatures and permissions.
export const LPK_API_VERSION = 1;

// Both the isolated host and the transitional session loader publish the same
// version. Keep module objects themselves live: SDK stores must remain shared.
export function createLpkApi(modules) {
  const shared = Object.freeze({ ...modules });
  const api = {
    require(name) {
      if (typeof name !== 'string' || !Object.prototype.hasOwnProperty.call(shared, name)) {
        throw new Error(`module not available to installed apps: ${String(name)}`);
      }
      return shared[name];
    },
  };
  Object.defineProperty(api, 'apiVersion', { value: LPK_API_VERSION, enumerable: true });
  return api;
}

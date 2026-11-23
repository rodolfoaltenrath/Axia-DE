// Transitional transport: Rust owns the configured/effective/measured check policies.
const model = lord.nativeApps.create('sysinfo');
const request = value => model.request(JSON.stringify(value));
const catalog = request({ cmd: 'catalog' });
export const LIMITS = catalog.limits;
export const PENDING = catalog.pending;
export const PROBE_CHECKS = catalog.probeChecks;
export const evaluate = value => request({ cmd: 'evaluate', value });
export const evaluateProbe = value => request({ cmd: 'evaluateProbe', value });
export const evaluateBench = value => request({ cmd: 'evaluateBench', value });

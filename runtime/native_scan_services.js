// Existing scanner/print APIs retain LAN validation, helper confinement and the
// system print dialog. Rust decides the device, settings and save sequence.
export function scanServices(print) {
  return {
    'scanner.discover': { run: () => print.discover() },
    'scanner.caps': { run: ([device]) => print.scanCaps(device) },
    'scanner.scan': { run: ([device, options]) => print.scan(device, options) },
    'print.open': { run: ([source]) => print.open(source) },
  };
}

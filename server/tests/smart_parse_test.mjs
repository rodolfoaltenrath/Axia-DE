// smart_parse_test.mjs — smartctl JSON → the Disks/Monitor model (sdk/smartparse.js).
//
// Fixtures in tests/smart/:
//   real smartctl 7.4 output captured on VM 119 (Debian 13):
//     ata-qemu-x.json (smartctl -j -x, QEMU SATA disk), ata-qemu-health.json (the smart-health
//     command), scsi-qemu-x.json (virtio-scsi disk: no SMART), scsi-qemu-health.json (smart-health on
//     it: -H prints "passed", -i says SMART is not available), usb-unknown-bridge.json (a USB
//     gadget smartctl does not know), usb-sat-nodata.json (the same retried with -d sat),
//     ata-test-started.json (smartctl -j -t short), ata-qemu-x-selftests.json (the same disk after
//     two short self-tests run from the Disks app: the self-test log)
//   the same smartctl 7.4 schema with edited values (no such drive on the test machines):
//     ata-ssd-worn.json, ata-hdd-failing.json, ata-selftest-running.json, ata-standby.json,
//     nvme-x.json, nvme-failing-testing.json
//
//   node tests/smart_parse_test.mjs        (from server/; run_suites.sh runs it when node exists)
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const { parseSmartctl, summarize, summaryFromOwn, prettyAttrName, ATTR_KEYS, HEALTH_RANK } = await import(path.join(here, '../../sdk/smartparse.js'));
const load = (f) => JSON.parse(fs.readFileSync(path.join(here, 'smart', f), 'utf8'));

let n = 0, bad = 0;
const eq = (name, got, want) => {
  n++;
  const g = JSON.stringify(got), w = JSON.stringify(want);
  if (g !== w) { bad++; console.log(`✗ ${name}: want ${w}, got ${g}`); }
};

// ---- real: QEMU SATA disk, full report ----
let m = parseSmartctl(load('ata-qemu-x.json'));
eq('qemu kind', m.kind, 'ata');
eq('qemu model', m.model, 'QEMU HARDDISK');
eq('qemu serial', m.serial, 'QM00015');
eq('qemu supported', m.supported, true);
eq('qemu passed', m.passed, true);
eq('qemu health', m.health, 'ok');
eq('qemu reasons', m.reasons, []);
eq('qemu temp', m.temp, 31);
eq('qemu hours', m.hours, 1);
eq('qemu cycles', m.cycles, 0);
eq('qemu reallocated', m.reallocated, 0);
eq('qemu pending (absent)', m.pending, null);
eq('qemu attrs', m.attrs.map((a) => a.id), [1, 3, 4, 5, 9, 12, 190]);
eq('qemu attr names', m.attrs.map((a) => a.key), ['a1', 'a3', 'a4', 'a5', 'a9', 'a12', 'a190']);
eq('qemu attr 190 raw string', m.attrs[6].raw, '31 (Min/Max 31/31)');
eq('qemu attr 190 count', m.attrs[6].count, 31);
eq('qemu prefail', m.attrs[0].prefail, true);
eq('qemu errors', m.errors, { count: 0, list: [] });
eq('qemu self-test', [m.selfTest.supported, m.selfTest.running, m.selfTest.polling.short, m.selfTest.polling.long], [true, false, 2, 54]);
eq('qemu self-tests', m.selfTests, []);
eq('qemu no wear (a disk)', m.wear, null);
eq('qemu no history', m.tempHistory, null);
eq('qemu exit', m.exit, 4);
eq('qemu standby', m.standby, false);

// ---- real: after two short self-tests ----
m = parseSmartctl(load('ata-qemu-x-selftests.json'));
eq('self-test log', m.selfTests.map((x) => [x.type, x.passed, x.hours, x.result]), [['short', true, 4660, 'Completed without error'], ['short', true, 4660, 'Completed without error']]);
eq('self-test log health', m.health, 'ok');

// ---- real: the smart-health command on the same disk ----
m = parseSmartctl(load('ata-qemu-health.json'));
eq('health cmd', [m.health, m.temp, m.hours, m.attrs.length, m.selfTest.running], ['ok', 31, 1, 7, false]);
eq('summary', summarize(m, 5), { health: 'ok', reasons: [], temp: 31, wear: null, hours: 1, standby: false, supported: true, t: 5 });

// ---- real: virtio-scsi disk, USB bridges ----
m = parseSmartctl(load('scsi-qemu-x.json'));
eq('scsi kind', m.kind, 'scsi');
eq('scsi unsupported', m.supported, false);
eq('scsi temp 0 is no value', m.temp, null);
eq('scsi health unknown', m.health, 'unknown');
eq('scsi model', m.model, 'QEMU QEMU HARDDISK');
m = parseSmartctl(load('scsi-qemu-health.json'));
eq('emulated disk: no verdict from -H', [m.passed, m.health, m.supported], [null, 'unknown', false]);
m = parseSmartctl(load('usb-unknown-bridge.json'));
eq('usb bridge', [m.kind, m.supported, m.health], [null, false, 'unknown']);
eq('usb bridge message', m.messages[0].text, '/dev/sdc: Unknown USB bridge [0x0525:0xa4a5 (0x612)]');
m = parseSmartctl(load('usb-sat-nodata.json'));
eq('usb sat no data', [m.kind, m.supported, m.health, m.exit], ['ata', false, 'unknown', 2]);
m = parseSmartctl(load('ata-test-started.json'));
eq('test started', [m.exit, m.messages.length], [0, 0]);

// ---- edited: worn SSD with reallocated and pending sectors ----
m = parseSmartctl(load('ata-ssd-worn.json'));
eq('worn health', m.health, 'warning');
eq('worn reasons', m.reasons, ['reallocated', 'pending', 'worn']);
eq('worn wear from device statistics', m.wear, { used: 93, source: 'devstat' });
eq('worn counts', [m.reallocated, m.pending, m.uncorrectable, m.hours, m.cycles], [8, 2, 0, 43210, 1520]);
eq('worn written', m.written, 93750000000 * 512);
eq('worn temp limits', [m.temp, m.tempMax, m.tempCrit], [38, 70, 75]);
eq('worn history', m.tempHistory, { interval: 1, values: [35, 36, null, 37, 38, 38, 37, 38] });
eq('worn errors', [m.errors.count, m.errors.list.length, m.errors.list[0].hours], [2, 2, 43100]);
eq('worn self-tests', m.selfTests.map((x) => [x.type, x.passed, x.hours]), [['long', false, 43150], ['short', true, 43000]]);
eq('past old-age failure is not a warning', m.reasons.includes('oldAgeNow'), false);
const noStats = load('ata-ssd-worn.json');
delete noStats.ata_device_statistics;
eq('wear from attribute 177', parseSmartctl(noStats).wear, { used: 93, source: 'attr177' });

// ---- edited: failing disk ----
m = parseSmartctl(load('ata-hdd-failing.json'));
eq('failing', m.health, 'failing');
eq('failing reasons', m.reasons, ['smartFailed', 'prefailNow', 'reallocated', 'pending', 'uncorrectable']);
eq('failing attr flagged', m.attrs.find((a) => a.id === 5).failed, 'now');
eq('failing temp 194 name', m.attrs.find((a) => a.id === 194).key, 'a194');

// ---- edited: self-test running, resting disk ----
m = parseSmartctl(load('ata-selftest-running.json'));
eq('running', [m.selfTest.running, m.selfTest.remaining], [true, 90]);
m = parseSmartctl(load('ata-standby.json'));
eq('standby', [m.standby, m.health, m.supported], [true, 'unknown', false]);

// ---- edited: NVMe ----
m = parseSmartctl(load('nvme-x.json'));
eq('nvme kind', m.kind, 'nvme');
eq('nvme health', m.health, 'ok');
eq('nvme temp + limits', [m.temp, m.tempMax, m.tempCrit], [41, 82, 85]);
eq('nvme wear', m.wear, { used: 3, source: 'nvme' });
eq('nvme hours, cycles', [m.hours, m.cycles], [5021, 812]);
eq('nvme bytes', [m.read, m.written], [12345678 * 512000, 23456789 * 512000]);
eq('nvme log', [m.nvme.spare, m.nvme.spareThreshold, m.nvme.unsafeShutdowns, m.nvme.mediaErrors, m.nvme.sensors], [100, 10, 37, 0, [41, 47]]);
eq('nvme errors (benign entries do not change health)', [m.errors.count, m.errors.list[0].text], [12, 'Invalid Field in Command']);
eq('nvme self-test', [m.selfTest.supported, m.selfTest.running], [true, false]);
eq('nvme self-tests', m.selfTests.map((x) => [x.type, x.passed, x.resultKey]), [['short', true, 'completed']]);
m = parseSmartctl(load('nvme-failing-testing.json'));
eq('nvme failing', m.health, 'failing');
eq('nvme failing reasons', m.reasons, ['smartFailed', 'nvmeReliability', 'nvmeSpare', 'mediaErrors', 'worn']);
eq('nvme test progress', [m.selfTest.running, m.selfTest.remaining], [true, 65]);

// ---- hostile input: nothing breaks, nothing is invented ----
for (const junk of [null, 42, 'x', [], {}, { smartctl: 7 }, { ata_smart_attributes: { table: [{ id: 'x' }, { id: 300 }, null] } },
  { nvme_smart_health_information_log: { temperature: 'hot' } }, { temperature: { current: 9999 } }]) {
  const r = parseSmartctl(junk);
  eq(`junk ${JSON.stringify(junk)} health`, ['unknown', 'ok'].includes(r.health), true);
  eq(`junk ${JSON.stringify(junk)} temp`, r.temp, null);
}
const long = parseSmartctl({ device: { protocol: 'ATA' }, model_name: 'x'.repeat(5000) });
eq('long strings cut', long.model.length, 80);

// ---- helpers ----
eq('pretty name', prettyAttrName('Unknown_SSD_Attribute'), 'Unknown SSD Attribute');
eq('keys are a<id>', ATTR_KEYS.every((k) => /^a\d{1,3}$/.test(k)), true);
eq('rank', HEALTH_RANK.failing > HEALTH_RANK.warning && HEALTH_RANK.warning > HEALTH_RANK.ok, true);
eq('own reader summary', summaryFromOwn({ kind: 'ata', health: 'warning', temp: 40, hours: 10 }, 1), { health: 'warning', reasons: [], temp: 40, wear: null, hours: 10, standby: false, supported: true, t: 1 });
eq('own reader nothing', summaryFromOwn(null), null);

console.log(`smart-parse-test: ${n} checks, ${bad} failures`);
process.exit(bad ? 1 : 0);

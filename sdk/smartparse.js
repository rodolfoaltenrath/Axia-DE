// smartparse.js — smartctl's JSON (smartmontools, `smartctl -j`) turned into one model the Disks
// app and the Monitor can show (see docs/SYSTEM.md, "SMART with smartmontools"). Pure functions,
// no lord.* and no React: tests/smart_parse_test.mjs runs them under node with real smartctl output.
//
//   const m = parseSmartctl(json, exitStatus)
//   m.health            'ok' | 'warning' | 'failing' | 'unknown'   (the UI says OK / Check / Failing)
//   m.reasons           why it is not 'ok': ['reallocated', 'pending', 'smartFailed', 'nvmeSpare', …]
//   m.attrs             ATA attributes [{ id, name, key, value, worst, thresh, raw, prefail, failed }]
//   m.nvme              NVMe health log fields (or null)
//   m.selfTest          { supported, running, remaining, polling: { short, long } } (minutes)
//   m.selfTests         past results [{ type, result, passed, hours }]
//   m.errors            { count, list: [{ n, hours, text }] }
//   m.tempHistory       { interval (minutes), values: [°C | null] } (ATA SCT; null if none)
//
// Everything a drive sends is outside data: numbers are checked, strings are only shown as text.

const num = (v) => (typeof v === 'number' && Number.isFinite(v) ? v : null);
const obj = (v) => (v && typeof v === 'object' && !Array.isArray(v) ? v : null);
const arr = (v) => (Array.isArray(v) ? v : []);
const str = (v, max = 200) => (typeof v === 'string' ? v.slice(0, max) : '');

// smartctl's attribute names → our plain-language strings (disks.attr.<key>). By name, not by id:
// vendors reuse ids, and smartctl's drive database already gives each one its real meaning.
const NAME_KEYS = {
  Raw_Read_Error_Rate: 'a1', Throughput_Performance: 'a2', Spin_Up_Time: 'a3', Start_Stop_Count: 'a4',
  Reallocated_Sector_Ct: 'a5', Reallocate_NAND_Blk_Cnt: 'a5', Seek_Error_Rate: 'a7', Seek_Time_Performance: 'a8',
  Power_On_Hours: 'a9', Power_On_Hours_and_Msec: 'a9', Spin_Retry_Count: 'a10', Calibration_Retry_Count: 'a11',
  Power_Cycle_Count: 'a12', Read_Soft_Error_Rate: 'a13', Helium_Level: 'a22', Program_Fail_Count: 'a171',
  Program_Fail_Cnt: 'a171', Erase_Fail_Count: 'a172', Ave_Block_Erase_Count: 'a173', Unexpect_Power_Loss_Ct: 'a174',
  Unsafe_Shutdown_Count: 'a174', Wear_Leveling_Count: 'a177', Used_Rsvd_Blk_Cnt_Tot: 'a179', Unused_Rsvd_Blk_Cnt_Tot: 'a180',
  Program_Fail_Cnt_Total: 'a181', Erase_Fail_Count_Total: 'a182', Runtime_Bad_Block: 'a183', SATA_Downshift_Count: 'a183',
  'End-to-End_Error': 'a184', Reported_Uncorrect: 'a187', Command_Timeout: 'a188', High_Fly_Writes: 'a189',
  Airflow_Temperature_Cel: 'a190', Temperature_Case: 'a190', 'G-Sense_Error_Rate': 'a191', 'Power-Off_Retract_Count': 'a192',
  Load_Cycle_Count: 'a193', Temperature_Celsius: 'a194', Temperature_Internal: 'a194', Hardware_ECC_Recovered: 'a195',
  Reallocated_Event_Count: 'a196', Current_Pending_Sector: 'a197', Offline_Uncorrectable: 'a198', UDMA_CRC_Error_Count: 'a199',
  CRC_Error_Count: 'a199', Multi_Zone_Error_Rate: 'a200', Percent_Lifetime_Remain: 'a202', Disk_Shift: 'a220',
  Loaded_Hours: 'a222', Load_Retry_Count: 'a223', Load_Friction: 'a224', Load_In_Time: 'a226', SSD_Life_Left: 'a231',
  Available_Reservd_Space: 'a232', Media_Wearout_Indicator: 'a233', Total_LBAs_Written: 'a241', Total_LBAs_Read: 'a242',
  Head_Flying_Hours: 'a240', Free_Fall_Sensor: 'a254',
};
export const ATTR_KEYS = [...new Set(Object.values(NAME_KEYS))];

// "Unknown_SSD_Attribute" → "Unknown SSD Attribute"
export const prettyAttrName = (n) => str(n, 64).replace(/_/g, ' ').trim();

// A count attribute's raw number: smartctl's raw string starts with it ("12 (Min/Max 0/40)").
function rawCount(a) {
  const s = str(a?.raw?.string, 64).trim();
  const m = /^(\d{1,15})\b/.exec(s);
  if (m) return Number(m[1]);
  const v = num(a?.raw?.value);
  return v != null && v >= 0 ? v : null;
}

function parseAttrs(j) {
  return arr(obj(j.ata_smart_attributes)?.table).filter((a) => obj(a) && num(a.id) != null && a.id > 0 && a.id < 256).slice(0, 64).map((a) => {
    const name = str(a.name, 64);
    return {
      id: a.id,
      name,
      key: NAME_KEYS[name] || null,
      value: num(a.value), worst: num(a.worst), thresh: num(a.thresh),
      raw: str(a?.raw?.string, 64) || (num(a?.raw?.value) != null ? String(a.raw.value) : ''),
      count: rawCount(a),
      prefail: !!a?.flags?.prefailure,
      failed: a.when_failed === 'now' ? 'now' : a.when_failed === 'past' ? 'past' : '',
    };
  });
}

const byKey = (attrs, ...keys) => attrs.find((a) => keys.includes(a.key)) || null;

// Percentage of the rated life already used (SSD wear), from the most reliable source available.
function wearOf(j, attrs, nvme) {
  if (nvme && nvme.used != null) return { used: nvme.used, source: 'nvme' };
  for (const p of arr(obj(j.ata_device_statistics)?.pages)) {
    for (const e of arr(p?.table)) {
      if (/Percentage Used Endurance Indicator/i.test(str(e?.name)) && num(e?.value) != null && e.value >= 0 && e.value <= 255) return { used: e.value, source: 'devstat' };
    }
  }
  const scsi = num(j.scsi_percentage_used_endurance_indicator);
  if (scsi != null) return { used: scsi, source: 'scsi' };
  // normalized values that count down from 100 as the flash wears out
  const a = byKey(attrs, 'a231', 'a233', 'a202', 'a177');
  if (a && a.value != null && a.value <= 100 && a.value >= 0) return { used: 100 - a.value, source: `attr${a.id}` };
  return null;
}

function parseNvme(j) {
  const h = obj(j.nvme_smart_health_information_log);
  if (!h) return null;
  const units = (v) => (num(v) != null ? v * 512000 : null);   // data units are 1000 × 512 bytes
  return {
    criticalWarning: num(h.critical_warning) ?? 0,
    temp: num(h.temperature),
    spare: num(h.available_spare), spareThreshold: num(h.available_spare_threshold),
    used: num(h.percentage_used),
    read: units(h.data_units_read), written: units(h.data_units_written),
    hostReads: num(h.host_reads), hostWrites: num(h.host_writes),
    busyMinutes: num(h.controller_busy_time),
    cycles: num(h.power_cycles), hours: num(h.power_on_hours),
    unsafeShutdowns: num(h.unsafe_shutdowns), mediaErrors: num(h.media_errors),
    errorLogEntries: num(h.num_err_log_entries),
    warningTempMinutes: num(h.warning_temp_time), criticalTempMinutes: num(h.critical_comp_time),
    sensors: arr(h.temperature_sensors).map(num).filter((v) => v != null).slice(0, 8),
  };
}

const ATA_TEST_RUNNING = (v) => v != null && v >> 4 === 15;      // status 0xF?: in progress

function parseSelfTest(j, nvme) {
  const polling = obj(obj(obj(j.ata_smart_data)?.self_test)?.polling_minutes);
  const st = obj(obj(obj(j.ata_smart_data)?.self_test)?.status);
  const caps = obj(obj(j.ata_smart_data)?.capabilities);
  const res = {
    supported: false, running: false, remaining: null,
    polling: { short: num(polling?.short), long: num(polling?.extended) },
  };
  if (caps) res.supported = !!caps.self_tests_supported;
  if (st) {
    res.running = ATA_TEST_RUNNING(num(st.value));
    if (res.running) res.remaining = num(st.remaining_percent) ?? (num(st.value) & 15) * 10;
    res.last = { result: str(st.string, 120), passed: st.passed !== false };
  }
  const nl = obj(j.nvme_self_test_log);
  if (nvme || nl) {
    const opt = obj(j.nvme_optional_admin_commands);
    res.supported = !!nl || (opt ? !!opt.self_test : false);
    const op = num(obj(nl?.current_self_test_operation)?.value);
    if (op) {
      res.running = true;
      const done = num(nl.current_self_test_completion_percent);
      res.remaining = done != null ? Math.max(0, 100 - done) : null;
    }
    const ext = num(j.nvme_extended_self_test_time ?? obj(j.nvme_self_test_time)?.extended);
    if (ext != null) res.polling.long = ext;
  }
  return res;
}

const NVME_RESULT = { 0: 'completed', 1: 'abortedCommand', 2: 'abortedReset', 3: 'abortedRemoved', 4: 'abortedFormat',
  5: 'fatal', 6: 'failedUnknownSegment', 7: 'failedSegments', 8: 'abortedUnknown', 9: 'abortedSanitize' };

function parseSelfTests(j) {
  const out = [];
  const log = obj(j.ata_smart_self_test_log);
  const tab = arr(obj(log?.extended)?.table).length ? obj(log.extended).table : arr(obj(log?.standard)?.table);
  for (const e of tab.slice(0, 32)) {
    if (!obj(e)) continue;
    out.push({
      type: /extended|long/i.test(str(e.type?.string)) ? 'long' : /short/i.test(str(e.type?.string)) ? 'short' : 'other',
      typeText: str(e.type?.string, 60),
      result: str(e.status?.string, 120),
      passed: e.status?.passed !== false,
      hours: num(e.lifetime_hours),
    });
  }
  for (const e of arr(obj(j.nvme_self_test_log)?.table).slice(0, 32)) {
    if (!obj(e)) continue;
    const code = num(e.self_test_code?.value), r = num(e.self_test_result?.value);
    out.push({
      type: code === 1 ? 'short' : code === 2 ? 'long' : 'other',
      typeText: str(e.self_test_code?.string, 60),
      result: str(e.self_test_result?.string, 120),
      resultKey: r != null ? NVME_RESULT[r] || null : null,
      passed: r === 0,
      hours: num(e.power_on_hours),
    });
  }
  return out;
}

function parseErrors(j, nvme) {
  const log = obj(j.ata_smart_error_log);
  const part = obj(log?.extended) || obj(log?.summary);
  if (part) {
    return {
      count: num(part.count) ?? 0,
      list: arr(part.table).slice(0, 16).filter(obj).map((e) => ({
        n: num(e.error_number), hours: num(e.lifetime_hours), text: str(e.error_description, 160),
      })),
    };
  }
  const ne = obj(j.nvme_error_information_log);
  if (ne || nvme) {
    return {
      count: nvme?.errorLogEntries ?? num(ne?.unread) ?? 0,
      list: arr(ne?.table).slice(0, 16).filter(obj).map((e) => ({
        n: num(e.error_count), hours: null, text: str(e.status_field?.string, 160),
      })),
    };
  }
  return { count: 0, list: [] };
}

function parseTempHistory(j) {
  const h = obj(j.ata_sct_temperature_history);
  if (!h) return null;
  const values = arr(h.table).slice(0, 1024).map((v) => (num(v) != null && v > -40 && v < 150 ? v : null));
  if (!values.some((v) => v != null)) return null;
  return { interval: num(h.logging_interval_minutes) ?? num(h.sampling_period_minutes) ?? 1, values };
}

function tempLimits(j) {
  const t = obj(j.temperature) || {};
  const sct = obj(obj(j.ata_sct_temperature_history)?.temperature) || {};
  const max = num(t.op_limit_max) ?? num(sct.op_limit_max) ?? num(j.nvme_warning_composite_temperature_threshold) ?? null;
  const crit = num(t.critical_limit_max) ?? num(sct.limit_max) ?? num(j.nvme_critical_composite_temperature_threshold) ?? null;
  const ok = (v) => (v != null && v >= 30 && v <= 150 ? v : null);
  return { max: ok(max), crit: ok(crit) };
}

const STANDBY = /\b(STANDBY|SLEEP|IDLE(_[A-Z])?) mode\b/;

export function parseSmartctl(j, exit = 0) {
  j = obj(j) || {};
  const sc = obj(j.smartctl) || {};
  const messages = arr(sc.messages).filter(obj).map((m) => ({ text: str(m.string, 200), severity: str(m.severity, 16) })).slice(0, 8);
  const status = num(sc.exit_status) ?? num(exit) ?? 0;
  const dev = obj(j.device);
  const protocol = str(dev?.protocol, 16);
  const kind = protocol === 'NVMe' ? 'nvme' : protocol === 'ATA' ? 'ata' : protocol === 'SCSI' ? 'scsi' : null;
  const nvme = parseNvme(j);
  const attrs = parseAttrs(j);
  const smartAvail = obj(j.smart_support) ? j.smart_support.available !== false : !!nvme || attrs.length > 0;
  const standby = !obj(j.smart_status) && messages.some((m) => STANDBY.test(m.text));
  const temp = num(obj(j.temperature)?.current) ?? nvme?.temp ?? null;
  const lim = tempLimits(j);
  const a5 = byKey(attrs, 'a5'), a197 = byKey(attrs, 'a197'), a198 = byKey(attrs, 'a198');
  const m = {
    kind,
    model: str(j.model_name, 80) || str(j.scsi_model_name, 80) || [str(j.scsi_vendor, 40), str(j.scsi_product, 40)].filter(Boolean).join(' '),
    serial: str(j.serial_number, 64),
    firmware: str(j.firmware_version, 40) || str(j.scsi_revision, 40),
    capacity: num(obj(j.user_capacity)?.bytes) ?? num(j.nvme_total_capacity),
    rotation: num(j.rotation_rate),
    supported: !!kind && smartAvail,
    enabled: obj(j.smart_support) ? j.smart_support.enabled !== false : true,
    standby,
    // a device that says it has no SMART (an emulated disk) gets no verdict, whatever -H printed
    passed: obj(j.smart_status) && !(obj(j.smart_support) && j.smart_support.available === false) ? j.smart_status.passed !== false : null,
    temp: temp != null && temp > -40 && temp < 150 && !(kind === 'scsi' && temp === 0) ? temp : null,
    tempMax: lim.max, tempCrit: lim.crit,
    hours: num(obj(j.power_on_time)?.hours) ?? nvme?.hours ?? null,
    cycles: num(j.power_cycle_count) ?? nvme?.cycles ?? null,
    reallocated: a5 ? a5.count : null,
    pending: a197 ? a197.count : null,
    uncorrectable: a198 ? a198.count : null,
    grownDefects: num(j.scsi_grown_defect_list),
    attrs,
    nvme,
    wear: null,
    read: nvme?.read ?? null,
    written: nvme?.written ?? null,
    selfTest: parseSelfTest(j, nvme),
    selfTests: parseSelfTests(j),
    errors: parseErrors(j, nvme),
    tempHistory: parseTempHistory(j),
    messages,
    exit: status,
    health: 'unknown',
    reasons: [],
  };
  if (!nvme) {
    const lbaW = byKey(attrs, 'a241'), lbaR = byKey(attrs, 'a242');
    if (lbaW && lbaW.count != null) m.written = lbaW.count * 512;
    if (lbaR && lbaR.count != null) m.read = lbaR.count * 512;
  }
  m.wear = wearOf(j, attrs, nvme);
  Object.assign(m, judge(m));
  return m;
}

// health from what the drive says about itself; never guessed when there is no data
export function judge(m) {
  const fail = [], warn = [];
  if (m.passed === false) fail.push('smartFailed');
  for (const a of m.attrs) {
    if (a.failed === 'now' && a.prefail) fail.push('prefailNow');
    else if (a.failed === 'now') warn.push('oldAgeNow');
    else if (a.failed === 'past' && a.prefail) warn.push('prefailPast');
  }
  if (m.reallocated > 0) warn.push('reallocated');
  if (m.pending > 0) warn.push('pending');
  if (m.uncorrectable > 0) warn.push('uncorrectable');
  if (m.grownDefects > 0) warn.push('grownDefects');
  const n = m.nvme;
  if (n) {
    const w = n.criticalWarning;
    if (w & 0x04) fail.push('nvmeReliability');
    if (w & 0x08) fail.push('nvmeReadOnly');
    if (w & 0x01 || (n.spare != null && n.spareThreshold != null && n.spare < n.spareThreshold)) warn.push('nvmeSpare');
    if (w & 0x02) warn.push('nvmeTemp');
    if (w & 0x10) warn.push('nvmeBackup');
    if (n.mediaErrors > 0) warn.push('mediaErrors');
  }
  if (m.wear && m.wear.used >= 90) warn.push('worn');
  if (m.passed == null && m.exit & 8) fail.push('smartFailed');     // smartctl: "DISK FAILING"
  const reasons = [...new Set([...fail, ...warn])];
  const known = m.passed != null || !!n || m.attrs.length > 0;
  return { health: fail.length ? 'failing' : warn.length ? 'warning' : known ? 'ok' : 'unknown', reasons };
}

// The little summary the Monitor keeps: { health, reasons, temp, wear, hours, t }
export function summarize(m, t = Date.now()) {
  return { health: m.health, reasons: m.reasons, temp: m.temp, wear: m.wear ? m.wear.used : null, hours: m.hours, standby: m.standby, supported: m.supported, t };
}

// Summary from LordOS's own SMART reader (disk-smart: smart.c), used when smartmontools is missing.
export function summaryFromOwn(s, t = Date.now()) {
  if (!obj(s)) return null;
  const map = { ok: 'ok', warning: 'warning', failing: 'failing' };
  return { health: map[s.health] || 'unknown', reasons: [], temp: num(s.temp) != null && s.temp >= 0 ? s.temp : null,
    wear: num(s.used) != null && s.kind === 'nvme' ? s.used : null, hours: num(s.hours) != null && s.hours >= 0 ? s.hours : null, standby: false, supported: true, t };
}

// health order, to notice when a disk gets worse
export const HEALTH_RANK = { unknown: 0, ok: 1, warning: 2, failing: 3 };

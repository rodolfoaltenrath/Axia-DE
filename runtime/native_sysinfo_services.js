// Bounded platform transport. Rust owns the pages, commands, deadlines,
// comparisons, export contents and sharing consent. No application UI lives here.
export function sysinfoServices({ systemInfo, codeStatus, admin, session, fs, userDirs, wm, workArea, launchApp, bluetooth }) {
  const info = () => lord.sysinfo;
  const optional = read => { try { return read() ?? null; } catch (_) { return null; } };
  const saved = () => [0, 1, 2, 3].map(n => optional(() => info()?.bench?.saved(n)));
  const readTestbench = () => { try { return JSON.parse(fs.read(`${fs.home()}/.local/state/axia/testbench.json`)); } catch (_) { return null; } };
  const collectLines = async action => {
    const lines = [];
    await admin.run(action, '', line => { if (lines.length < 4096) lines.push(String(line).slice(0, 8192)); }, { quiet: true });
    return lines;
  };
  return {
    'sysinfo.snapshot': { run: () => ({ report: info()?.report(), markdown: info()?.markdown() || '',
      facts: optional(() => info()?.facts()), probe: optional(() => info()?.probe()), community: optional(() => info()?.community()), info: systemInfo(), code: optional(codeStatus), stats: optional(() => lord.stats()),
      saved: saved(), bench: optional(() => info()?.bench?.poll()), boots: optional(() => info()?.bench?.boots()), bootTest: optional(() => info()?.bench?.bootTest()),
      canAdmin: !!systemInfo()?.admin && admin.available(), canPower: session.canPower(),
      testbench: readTestbench(), case: optional(() => lord.fan?.caseConfig?.()) }) },
    'sysinfo.local': { run: () => ({ devices: info()?.peripherals() || [], bluetooth: bluetooth.get() }) },
    'sysinfo.serials': { async run([window], owner) {
      if (!systemInfo()?.admin || !admin.available()) throw Error('admin.unavailable');
      const lease = { window, active: true, protected: false };
      owner.retain('sysinfo-serials', lease, value => { value.active = false; if (value.protected) wm.setProtected(value.window, false); });
      const lines = await collectLines('hw-serial-numbers');
      if (!owner.alive() || !lease.active) return null;
      // Protect before any serial data is returned to the native view.
      wm.setProtected(window, true);
      lease.protected = true;
      return lines;
    } },
    'sysinfo.hideSerials': { run: (_, owner) => { owner.release('sysinfo-serials'); return null; } },
    'sysinfo.modules': { run: () => collectLines('hw-memory-modules') },
    'sysinfo.benchStart': { run: ([mode, seconds]) => info()?.bench?.start(mode, seconds) === true },
    'sysinfo.benchPoll': { run: () => ({ bench: info()?.bench?.poll(), saved: saved() }) },
    'sysinfo.benchCancel': { run: () => info()?.bench?.cancel() },
    'sysinfo.probeStart': { run: () => !!lord.apps.launchPackage('sysprobe') },
    'sysinfo.probePoll': { run: () => info()?.probe() },
    'sysinfo.testbench': { run: ([open]) => { if (open) launchApp('desktop:axia-testbench'); return readTestbench(); } },
    'sysinfo.bootStart': { async run([n]) { if (![3, 5, 10].includes(n)) throw Error('app.invalidRequest'); await admin.run('boot-test', String(n)); return info()?.bench?.bootTest(); } },
    'sysinfo.bootCancel': { async run() { await admin.run('boot-test-cancel', ''); return info()?.bench?.bootTest(); } },
    'sysinfo.restart': { run: () => session.restart() },
    'sysinfo.scene': { run([on], owner) {
      owner.release('sysinfo-scene');
      if (!on) return null;
      // WM geometry/loading remains a host operation until shell composition migrates.
      const a = workArea();
      const id = wm.open('sysinfo', { instance: 'lordbench-scene', args: { scene: true },
        frame: { x: Math.round(a.x + (a.w - 760) / 2), y: Math.round(a.y + (a.h - 480) / 2), w: 760, h: 480 } });
      let timer, tries = 0;
      const go = () => {
        if (!owner.alive() || !wm.get(id)) return;
        if (!wm.scriptMove(id, { rx: Math.max(0, Math.floor((a.w - 760) / 2) - 20), ry: Math.max(0, a.h - 520), periodMs: 6000 }) && ++tries < 50) timer = setTimeout(go, 50);
      };
      timer = setTimeout(go, 50);
      owner.retain('sysinfo-scene', id, window => { clearTimeout(timer); wm.stopScript(); wm.close(window, true); });
      return id;
    } },
    'sysinfo.save': { run([files]) {
      const dir = userDirs().documents;
      try { fs.mkdir(dir); } catch (_) { /* existing folder */ }
      // The native app supplies the exact previewed bytes and a fixed safe basename.
      if (!Array.isArray(files) || !files.length || files.length > 2 || files.some(f => !/^(?:lordos-(?:hwreport|report)|lordbench)-[a-zA-Z0-9-]+\.(?:json|html|md)$/.test(f.name) || typeof f.text !== 'string' || f.text.length > 4 * 1024 * 1024)) throw Error('app.invalidRequest');
      const names = files.map(f => f.name);
      const exists = path => { try { return !!fs.stat(path); } catch (_) { return false; } };
      if (names.some(name => exists(`${dir}/${name}`))) throw Error('sysinfo.exportExists');
      for (let i = 0; i < files.length; i++) fs.write(`${dir}/${names[i]}`, files[i].text);
      return { dir, names };
    } },
    'sysinfo.openFile': { run: ([path]) => lord.apps.launchWith('', path) },
  };
}

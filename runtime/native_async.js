// Promise transport for compiled applications. Rust owns request policy and UI;
// each window owns its jobs. Closing it cancels publication of late results.
export function createNativeAsync(methods) {
  const owners = new Map();
  let sequence = 0;
  return {
    open(id, allowed) {
      if (owners.has(id)) throw new Error('nativeAsync.duplicateWindow');
      owners.set(id, { allowed: new Set(allowed), jobs: new Map(), events: [], cleanups: [] });
    },
    close(id) {
      const owner = owners.get(id);
      if (!owner) return;
      owners.delete(id);
      for (const stop of owner.cleanups) { try { stop(); } catch (error) { console.error('native async cleanup:', error); } }
      for (const job of owner.jobs.values()) if (job.ready) job.dispose?.(job.result);
      owner.jobs.clear(); owner.events.length = 0;
    },
    start(id, method, args = []) {
      const owner = owners.get(id), spec = methods[method];
      if (!owner || !owner.allowed.has(method) || !spec) throw new Error('app.invalidRequest');
      if (owner.jobs.size >= 32) throw new Error('nativeAsync.tooManyRequests');
      const token = ++sequence;
      const job = { ready: false, dispose: spec.dispose, offset: 0 };
      owner.jobs.set(token, job);
      const context = {
        id,
        emit(event) {
          if (owners.get(id) !== owner) return;
          if (owner.events.length >= 256) owner.events.shift();
          owner.events.push(event);
        },
        cleanup(stop) { if (owners.get(id) === owner) owner.cleanups.push(stop); else stop(); },
        alive: () => owners.get(id) === owner,
      };
      Promise.resolve().then(() => {
        if (!context.alive() || owner.jobs.get(token) !== job) return undefined;
        job.started = true;
        return spec.run(args, context);
      }).then(result => {
        if (!context.alive() || owner.jobs.get(token) !== job) { if (job.started) spec.dispose?.(result); return; }
        job.ready = true; job.result = result ?? null;
      }, error => {
        if (!context.alive() || owner.jobs.get(token) !== job) return;
        job.ready = true; job.error = String(error?.message || error || 'app.invalidRequest');
      });
      return token;
    },
    take(id, tokens, discard = []) {
      const owner = owners.get(id);
      if (!owner) return [];
      for (const token of discard) {
        const job = owner.jobs.get(token);
        if (job?.ready) job.dispose?.(job.result);
        owner.jobs.delete(token);
      }
      const ready = [];
      let budget = 128 * 1024; // below the native host's 4 MiB JSON limit, including escaping
      for (const token of tokens || []) {
        const job = owner.jobs.get(token);
        if (!job?.ready) continue;
        if (job.error) { owner.jobs.delete(token); ready.push({ token, error: job.error }); continue; }
        job.encoded ??= JSON.stringify(job.result);
        if (job.offset === 0 && job.encoded.length <= budget) {
          owner.jobs.delete(token); ready.push({ token, value: job.result }); budget -= job.encoded.length;
        } else {
          if (budget < 2) break;
          let end = Math.min(job.encoded.length, job.offset + budget);
          const last = job.encoded.charCodeAt(end - 1);
          if (last >= 0xd800 && last <= 0xdbff) end--;
          const part = job.encoded.slice(job.offset, end);
          job.offset = end; budget -= part.length;
          const done = end === job.encoded.length;
          ready.push({ token, part, done });
          if (done) owner.jobs.delete(token);
        }
        if (budget < 2) break;
      }
      return ready;
    },
    events(id) {
      const owner = owners.get(id);
      return owner ? owner.events.splice(0) : [];
    },
  };
}

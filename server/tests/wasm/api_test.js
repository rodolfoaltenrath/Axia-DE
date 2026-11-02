// api_test.js — the LordOS WebAssembly global (src/ui/js_wasm.c). Run by build/wasm-js-test.
let fails = 0, oks = 0;
const ok = (cond, what) => { if (cond) oks++; else { fails++; print('FAIL ' + what); } };
const throwsAs = (fn, cls, what) => { try { fn(); ok(false, what + ' (did not throw)'); } catch (e) { ok(e instanceof cls, what + ' (threw ' + e + ')'); } };

const api = readFile('tests/wasm/api.wasm'), demo = readFile('tests/wasm/demo.wasm');
ok(WebAssembly.validate(api), 'validate api');
ok(!WebAssembly.validate(new Uint8Array([0, 97, 115, 109, 2, 0, 0, 0])), 'validate bad');
throwsAs(() => new WebAssembly.Module(new Uint8Array([1, 2, 3])), WebAssembly.CompileError, 'CompileError');
ok(new WebAssembly.CompileError('x') instanceof Error, 'CompileError extends Error');

const m = new WebAssembly.Module(api);
ok(WebAssembly.Module.exports(m).length === 14, 'Module.exports');
ok(JSON.stringify(WebAssembly.Module.imports(m)[2]) === '{"module":"js","name":"g","kind":"global"}', 'Module.imports');
throwsAs(() => new WebAssembly.Instance(m, {}), TypeError, 'missing imports');
throwsAs(() => new WebAssembly.Instance(m, { js: { throws() {}, add3: 1, g: 0, mem: 0 } }), WebAssembly.LinkError, 'LinkError');

const mem = new WebAssembly.Memory({ initial: 1, maximum: 10 });
const g = new WebAssembly.Global({ value: 'i32', mutable: true }, 7);
const inst = new WebAssembly.Instance(m, { js: { throws() { throw new Error('desde JS'); }, add3: (x) => x + 3, g, mem } });
const e = inst.exports;
ok(Object.isFrozen(e) && Object.getPrototypeOf(e) === null, 'exports frozen and without prototype');
ok(typeof e.big === 'function' && e.big.length === 1, 'exported function');
ok(e.big(5n) === 15n, 'i64 ↔ BigInt');
throwsAs(() => e.big(5), TypeError, 'i64 with Number');
ok(JSON.stringify(e.pair(1, 2)) === '[2,1]', 'multi-value');
const obj = { hola: 1 };
ok(e.ext(obj) === obj && e.ext(null) === null && e.ext(undefined) === undefined, 'externref');
try { e.callThrows(); ok(false, 'JS exception'); } catch (x) { ok(x.message === 'desde JS', 'the JS exception crosses wasm'); }
ok(e.useAdd(4) === 7, 'JS import');
e.incG(); e.incG();
ok(g.value === 9, 'imported mutable global');
throwsAs(() => e.trap(), WebAssembly.RuntimeError, 'trap → RuntimeError');
ok(e.answer instanceof WebAssembly.Global && e.answer.value === 42, 'exported global');
throwsAs(() => { e.answer.value = 1; }, TypeError, 'immutable global');
e.g64.value = 1n << 40n;
ok(e.g64.value === 1n << 40n, 'global i64');
ok(e.tbl.length === 2 && e.tbl.get(0) === e.f1 && e.tbl.get(1) === null, 'table and function identity');
e.tbl.set(1, e.f1);
ok(e.callTbl(1) === 11, 'call_indirect through an entry set from JS');
throwsAs(() => e.callTbl(5), WebAssembly.RuntimeError, 'call_indirect outside the table');
throwsAs(() => e.tbl.set(0, () => 1), TypeError, 'funcref table with a JS function');
ok(e.tbl.grow(3) === 2 && e.tbl.length === 5, 'Table.grow');

// memory: the buffer is the real memory; growing detaches it
const b1 = mem.buffer;
ok(b1 === mem.buffer && b1.byteLength === 65536, 'Memory.buffer');
e.store(100, 77);
ok(new Uint8Array(b1)[100] === 77, 'wasm writes and JS reads without copying');
ok(e.grow(2) === 1, 'memory.grow from wasm');
ok(b1.byteLength === 0, 'the old buffer was detached');
ok(mem.buffer.byteLength === 3 * 65536 && new Uint8Array(mem.buffer)[100] === 77, 'new buffer with the data');
ok(mem.grow(1) === 3 && mem.buffer.byteLength === 4 * 65536, 'Memory.grow from JS');
throwsAs(() => mem.grow(100), RangeError, 'Memory.grow beyond the maximum');

// the C module (demo.c), with promises
let log = '';
let demoInst;
WebAssembly.instantiate(demo, { env: { log: (p, n) => { log = String.fromCharCode(...new Uint8Array(demoInst.exports.memory.buffer, p, n)); } } })
  .then(({ module, instance }) => {
    demoInst = instance;
    ok(module instanceof WebAssembly.Module && instance instanceof WebAssembly.Instance, 'instantiate(bytes)');
    ok(instance.exports.fib(20) === 6765, 'fib from JS');
    ok(instance.exports.greet(0) === 13 && log === 'hola desde C!', 'import that reads memory');
    const t0 = Date.now();
    const p = instance.exports.primes(2000000);
    print('primes(2000000) = ' + p + ' in ' + (Date.now() - t0) + ' ms (from JS)');
    return WebAssembly.instantiate(module, { env: { log() {} } });
  })
  .then((i2) => {
    ok(i2 instanceof WebAssembly.Instance, 'instantiate(Module)');
    return WebAssembly.compile(new Uint8Array([0, 1, 2]));
  })
  .then(() => ok(false, 'bad compile resolved'), (err) => ok(err instanceof WebAssembly.CompileError, 'compile rejects with CompileError'))
  .then(() => print((fails ? fails + ' FAILURES, ' : 'all good, ') + oks + ' checks'));

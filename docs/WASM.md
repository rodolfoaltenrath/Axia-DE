# WebAssembly in LordOS

LordOS ships its own WebAssembly interpreter, written in C with no dependencies. It doesn't use
wasm3, WAMR, V8 or Wasmtime. From JavaScript it is used through the standard `WebAssembly`
global, just like in a browser, so a native LordOS app (React) can carry modules compiled from
C, C++, Rust or Zig and call them.

| Piece | File |
|---|---|
| Decoder, validator and compiler to internal code | `server/src/wasm/wasm.c` |
| Interpreter, instances, memories, tables and globals | `server/src/wasm/wasm_run.c` |
| C API | `server/src/wasm/wasm.h` (internals: `wasm_int.h`) |
| `WebAssembly` global in QuickJS | `server/src/ui/js_wasm.c` |
| Tests | `server/tests/wasm_test.c`, `server/tests/wasm_js_test.c`, `server/tests/wasm/` |

## What it supports

It covers WebAssembly 2.0 without SIMD:
- **The full MVP:** integers and floats with the exact semantics of the specification (NaN,
  −0, rounding, division and conversion traps), memory, tables, `call_indirect`, globals,
  `start`, imports and exports.
- **Additions that clang and rustc emit today:**
  - sign-extension;
  - non-trapping float-to-int (`trunc_sat`);
  - multi-value (functions and blocks with several results or with parameters);
  - bulk memory (`memory.copy`, `memory.fill`, `memory.init`, `data.drop`, `table.init`,
    `elem.drop`, `table.copy`);
  - reference types (`funcref`/`externref` in tables, globals and parameters, `ref.null`,
    `ref.func`, `ref.is_null`, `table.get/set/size/grow/fill`, multiple tables, typed `select`);
  - import and export of mutable globals.

**Limits:**
- **Memory:** a single one, 32-bit (up to 4 GiB). It is reserved with `mmap` without taking RAM,
  so only the pages the module touches are used. Growing doesn't change its address.
- **Stack:** 20,000 nested wasm calls (`wasm_max_call_depth`) and 8 MiB of values. When exceeded,
  it raises the "call stack exhausted" trap and doesn't break the server.
- **Errors:** every error is a trap or a validation error, never a crash. After a trap, the
  instance can still be used.

**What it doesn't support (pending):**
- SIMD (`v128`);
- threads and shared memory;
- memory64 and multiple memories;
- exceptions (`try`/`throw`) and tail calls;
- GC, `extended-const` and relaxed SIMD;
- WASI: a module that imports `wasi_snapshot_preview1` can't be instantiated, unless the app
  provides those functions from JS.

## How it works

1. **One pass per function:** `wasm.c` validates the types with the algorithm from the
   specification's appendix and, in the same pass, generates internal code. Each instruction is
   the address of its handler (GCC/clang computed goto) followed by its immediates, already
   decoded: no LEB128 is read at run time.
2. **Branches resolved in advance:** the validator knows the exact stack height at each point,
   so every `br`, `br_if` and `br_table` comes out with its relative target and how many values
   to move. If there is nothing to move, the branch is a single addition to the instruction
   pointer. `i32.wrap_i64` and the `reinterpret`s generate no code.
3. **Dead code isn't generated:** what follows `br`, `return` or `unreachable` is validated (the
   specification's "polymorphic" stack), but not compiled.
4. **Stacks:**
   - a stack of 64-bit values and a stack of frames, shared by all calls;
   - wasm → wasm calls don't use the C stack;
   - a host function (a JS import) can call back into wasm.
5. **References:**
   - modules, instances, memories, tables and globals are reference-counted;
   - a table or global holds the functions of its own instance without adding references, which
     avoids the instance ↔ its table cycle;
   - if someone outside (JS, another instance) holds the table, the table holds its instance.

## From JavaScript (`WebAssembly`)

The global is the same as in browsers:
- **Functions:** `validate`, `compile` and `instantiate`. `instantiate` accepts bytes (and
  returns `{module, instance}`) or a `Module` (and returns the `Instance`), always with promises.
- **Classes:** `Module` (with `exports`, `imports` and `customSections`), `Instance` (with a
  frozen, prototype-less `exports`), `Memory` (`buffer`, `grow`), `Table` (`length`, `get`,
  `set`, `grow`) and `Global` (`value`, `valueOf`).
- **Errors:** `CompileError`, `LinkError` and `RuntimeError` are subclasses of `Error`. When the
  stack is exhausted, the error is `RangeError`.

**Exported functions:**
- they are real JS functions: `typeof` gives `"function"` and `call`, `apply` and `bind` work;
- the same wasm function always returns the same object: `exports.f === table.get(0)`.

**Value conversions:**
- i64 ↔ `BigInt`: passing a `Number` to an i64 parameter gives `TypeError`;
- f32: rounded with `Math.fround`;
- `externref` accepts any JS value;
- `funcref` accepts an exported wasm function or `null`;
- if a function has several results, it returns an array.

**Imports and exceptions:**
- an import can be any JS function;
- if that function throws, the same exception passes through wasm and reaches whoever called
  from JS.

**Memory and `Memory.buffer`:**
- `Memory.buffer` is an `ArrayBuffer` over the real memory, with no copy: what wasm writes, JS
  reads immediately;
- on growth (from wasm or with `grow`), the previous `ArrayBuffer` is detached (`byteLength` 0)
  and `buffer` gives a new one, as the specification requires;
- the memory never moves, so no pointer can be left dangling.

### Example in a LordOS app

`calc.c`:
```c
__attribute__((export_name("add"))) int add(int a, int b) { return a + b; }
```
```sh
clang --target=wasm32 -O2 -nostdlib -Wl,--no-entry -o calc.wasm calc.c
```

In the app (React):
```jsx
import { useEffect, useState } from 'react';
import { View, Text } from '@lordos/sdk';

export default function Calc() {
  const [r, setR] = useState(null);
  useEffect(() => {
    // bytes: lord.apps.packageFile('my.app', 'calc.wasm') if the .wasm ships in the app's
    // package (see NATIVE-APPS.md), or fs.read(path, { binary: true }) from the home folder.
    const bytes = lord.apps.packageFile('my.app', 'calc.wasm');
    WebAssembly.instantiate(bytes, {}).then(({ instance }) => setR(instance.exports.add(2, 3)));
  }, []);
  return <View><Text>{r === null ? '…' : `2 + 3 = ${r}`}</Text></View>;
}
```

With imports, for example a function that prints from C using the module's memory:
```js
let inst;
const imports = { env: { log: (ptr, len) => console.log(new TextDecoder().decode(new Uint8Array(inst.exports.memory.buffer, ptr, len))) } };
({ instance: inst } = await WebAssembly.instantiate(bytes, imports));
```

**Security:**
- a module only sees its own memory and the imports the app gives it;
- it has no access to the file system, the network or `lord.*` unless the app passes them as
  functions;
- every address is checked against the memory size before reading or writing.

**Memory cycles:** the QuickJS garbage collector doesn't see the references the interpreter
keeps to JS functions (imports) or to `externref` values. A cycle that goes through there, such
as an import that uses the instance importing it, stays alive as long as the JS context lives.
When the theme changes or the server shuts down, `js_wasm_shutdown` cuts those references and
everything is freed; `build/wasm-js-test` checks it with QuickJS's `assert`, which aborts if any
object is left alive.

## C API

```c
WasmError err;
WasmModule *m = wasm_module_new(bytes, len, &err);            // decodes, validates and compiles
WasmFunc *log = wasm_func_new_host(wasm_functype(ii, 2, NULL, 0), my_log, data, NULL);
WasmExtern imports[] = { { .kind = WASM_EXT_FUNC, .func = log } };
WasmInstance *inst = wasm_instantiate(m, imports, 1, &err);   // runs start
WasmExtern f = wasm_instance_export_find(inst, "add");
WasmVal args[2] = { { .i32 = 2 }, { .i32 = 3 } }, res[1];
if (wasm_call(f.func, args, res, &err)) printf("trap: %s\n", err.msg);
```

Import and export names can contain 0 bytes: `wasm_export_name(m, i, &len)` and
`wasm_instance_export_find_len`.

## Tests

```sh
sh server/tests/wasm/build.sh        # demo.c → demo.wasm (clang) and api.wat → api.wasm (wabt)
make -C server build/wasm-test build/wasm-js-test
cd server && ./build/wasm-test && ./build/wasm-js-test
```

- **`wasm-test`:** runs the module compiled from C (`tests/wasm/demo.c`, no libc). It tests
  recursion, i64, a sieve with 2 MB of memory, quicksort with `call_indirect`, CRC32, floats, a
  host import that reads memory, `memory.grow`, bulk memory, the 7/0 trap and infinite recursion
  (stack exhausted). At the end it checks the instance still works: 16 of 16.
- **`wasm-js-test`:** runs `tests/wasm/api_test.js` in QuickJS with the `WebAssembly` global: 38
  of 38 checks. When done it frees the context and verifies nothing is left alive.

### Official specification suite

These are the 90 `.wast` files from WebAssembly/spec's `test/core` at the `wg-2.0` tag (without
the `simd` folder). wabt converts them to JSON and binaries, and `wasm-test spec` runs them:

```sh
git clone https://github.com/WebAssembly/spec && cd spec && git checkout wg-2.0
mkdir json && for f in test/core/*.wast; do wast2json $f -o json/$(basename $f .wast).json; done
.../server/build/wasm-test spec json/*.json
```

Result on the VM: **26,290 of 26,290 assertions pass, 0 skipped**. It covers:
- `assert_return` with canonical and arithmetic NaNs;
- `assert_trap`, `assert_exhaustion`, `assert_invalid` and `assert_malformed` (the binary ones);
- `assert_unlinkable`, `assert_uninstantiable`, `register` and `linking`.

There is 1 that differs only in the error kind and is counted among those 26,290:
`memory_init.wast:190`. `data.drop` without a `datacount` section gives "malformed", which is
what the binary format requires. The reference interpreter expects "invalid" because it reads
the text without going through the binary.

Text assertions (`module_type: text`) test wabt's parser, not ours, and aren't counted.

## Performance (VM: Xeon Silver 4108 at 1.8 GHz, 1 core of 4 vCPUs)

| Test | wasm (LordOS) | Same code in JS (QuickJS) |
|---|---|---|
| Recursive `fib(30)` (1.6 million calls) | 66–90 ms | 322 ms |
| Prime sieve up to 2,000,000 | 173–237 ms | 540 ms |
| 100,000 JS → wasm calls | 18 ms | — |

The range depends on the VM's load, which had other builds running.

Compared with browser engines: an interpreter is 3 to 10 times slower than their JIT compiler,
which LordOS doesn't have. Even so, wasm beats the same code written in JS on QuickJS by 2 to 3.5
times. Instantiating `demo.wasm` (2.5 KB, with 2 MB of memory) takes no RAM until the module
touches that memory.

Left for later: fusing frequent sequences into a single instruction (`local.get` + `i32.add`,
comparison + `br_if`): it gives 20–40% more in other interpreters.

## Packages in the VM

Only for building the tests and running the suite; LordOS doesn't need them to work.

| Package | What for |
|---|---|
| `clang`, `lld` | compile `tests/wasm/demo.c` to wasm32 (`wasm-ld` is in `lld`) |
| `wabt` (1.0.36) | `wat2wasm` for `api.wat` and `wast2json` for the specification suite |
| `git`, `ca-certificates` | clone WebAssembly/spec (in `~/wasm-spec` on the VM) |

```sh
sudo apt-get install -y --no-install-recommends clang lld wabt git ca-certificates
```

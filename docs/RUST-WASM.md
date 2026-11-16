# Rust WebAssembly

`rust/axia-codecs/src/wasm/` replaces `server/src/wasm/wasm.c`, `wasm_run.c` and `wasm_int.h` (about 4,000
lines of C). `wasm.h` stays as the C contract; `js_wasm.c` (the `WebAssembly` global of QuickJS) calls it
unchanged. The C files are gone; there is no C fallback. `libaxia_shell.a`, which `axia-server` links,
already contains `axia-codecs`, so the server needs nothing new.

## What is where

| Piece | File |
|---|---|
| ABI types, errors, interned function types, reference counting, memories, tables, globals, module accessors | `mod.rs` |
| Binary decoder, validator and compiler to the internal code (`wasm_module_new`, `wasm_validate`) | `decode.rs` |
| Interpreter, `wasm_call`, instantiation, exports, stack unwinding | `exec.rs` |

The design is the C one on purpose, so behaviour can be compared byte for byte:

- Compiled code is a vector of 64-bit cells (opcode = the wasm byte, `0x100 + n` for the `0xFC` prefix and a
  few internal ones for branches and calls). The interpreter is a `match` loop over raw 64-bit stack slots.
  Operations on 32-bit values write only the low half, as the C union did.
- Memories reserve the full address range with `mmap` and `mprotect` it as they grow, so `wasm_memory_data`
  never moves (the JS `ArrayBuffer` relies on it). The value stack is one 1M-slot mapping.
- Objects are heap allocations behind raw pointers with the same reference counting, including the rule that
  keeps an instance and its table from keeping each other alive.
- Errors keep the exact messages (truncated to 159 bytes) and kinds. The decoder returns `Result` instead of
  `setjmp`/`longjmp`.
- `wasm_err` (variadic) cannot be defined in Rust, so it is now `static inline` in `wasm.h`.
- `wasm_max_call_depth` and `wasm_max_memory_pages` are still exported globals.
- The module is behind `cfg(not(feature = "secure-memory"))`: only the standalone server uses it.

As in the C version, the interpreter has one global machine (value stack and frames) and is meant for the
single JS thread; the unit tests take a lock for that reason.

## Validation

Done against the original C, compiled with every symbol renamed (`c_` prefix) and linked into the same
program, so both run in the same process on the same input:

- all 136 numeric instructions (including `0xFC 0..7`) on edge-case operands: zero, signs, powers of two,
  `INT_MIN`, NaNs, infinities, subnormals, values around the conversion limits;
- about 130,000 random typed programs and 750,000 mutated modules (blocks, loops, `if`, `br_table`, dead
  code, memory/table/global access, bulk operations, `call` and `call_indirect`, passive and active segments),
  comparing the decoder and validator error (kind and message), instantiation errors, results, traps and the
  final state of memory, globals and the table;
- linking (every kind of mismatch), host functions (results, `TRAP`, `HOST`, re-entering wasm), call depth
  limits, memory/table limits, and an object outliving its instance and module;
- the same runs against a debug build, which turns on overflow checks;
- the harness itself was tested by planting defects in the Rust (a shift mask, a `br_table` clamp, an inverted
  `select`, a rotate direction, one character of an error message): each was caught.

Result: no differences. Any NaN compares equal to any NaN (the payload of a NaN produced by arithmetic is
not specified).

`server/tests/wasm_test.c` and `wasm_js_test.c` now link `libaxia_codecs.a`. `wasm_test.c` passes, including
the `fib` and `primes` benchmarks. `wasm_js_test.c` needs `api.wasm` (built with `wat2wasm`, not available
where this was done), so it was not run against the Rust build; the C API it uses is the one covered above.

## Performance

About 25% slower than the C interpreter on the benchmarks in `wasm_test.c` (`fib(30)`: 117 ms in C, 147 ms in
Rust; `primes(2000000)`: 290 ms against 360 ms). The generated code spills the memory base to the stack and
re-materialises a couple of constants at each dispatch; `codegen-units=1` made no difference. It was not
worth complicating the loop for; if it matters, that is where to look.

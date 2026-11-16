# Rust disks

`rust/axia-codecs/src/disk.rs` and `smartctl.rs` replace `server/src/disk/disk.c` and
`smartctl.c`. The SMART parsers were already Rust (`smart.rs`). Those two C files are gone;
there is no C fallback.

## What is Rust and what is not

| Piece | Where | |
|---|---|---|
| Disks, partitions and mounts without root: `/sys/block`, the udev database, `/proc/self/mountinfo`, `/proc/swaps`; the system-disk set; the JSON of `lord.disks.list()`; `tools()`; the change notifications (epoll over mountinfo and a udev netlink socket) | `disk.rs` | Rust |
| Which names `smartctl` may be pointed at, the compaction of its JSON, the LOG pieces | `smartctl.rs` | Rust |
| ATA SMART and NVMe health log parsers | `smart.rs` | Rust (already) |
| `lord.disks` binding for QuickJS | `server/src/ui/js_disk.c` | C |
| `disk_admin.c`, `smartctl_admin.c`, `diskkinds.h` | `server/src/disk/` | C, **not compiled** |

The last row matters. The privileged helper no longer exists in the standalone desktop:
`server/src/platform/admin.c` is a stub (`admin_known()` is 0, `admin_send()` fails with
`ENOTSUP`), nothing registers `DISK_ADMIN_ACTIONS`, no build lists those files, and the Imager
that used the `disk_admin_*` API is gone. So the Disks app's format, erase, mount and SMART
buttons already answer `admin.unavailable`. The files were left as they are: porting
data-destroying root-side code that nothing builds, and that cannot be exercised without root and
a real block device, would add risk and no behaviour. They either come back with a replacement for
the helper or they get deleted; that is a decision for whoever owns the feature.

## Boundary

`server/src/disk/disk.h` is unchanged in substance and remains the contract: every function keeps
its name, arguments and ownership. Returned strings come from `malloc` and C releases them with
`free()`. `axia-server` reaches the symbols through `libaxia_shell.a`, which already contains
`axia-codecs`; the vault archive does not carry them (they sit behind
`#[cfg(not(feature = "secure-memory"))]` like the other helpers).

The state of the change notifications (the epoll descriptor and its two sources) is process-wide,
as it was, in atomics. Out of memory aborts the process instead of returning `NULL`, as Rust's
allocation does.

## Faithfulness

The port keeps what the C code did, quirks included, because callers see them: files are read with
one `read` into a fixed buffer, trailing blanks are trimmed, the udev database and mountinfo are
read in `fgets`-sized pieces (a longer line is split and the second piece is looked at as a line of
its own), `sscanf`'s token limits cut a long file system name and hand the rest to the next field,
values stop at a NUL, octal and `\xNN` escapes decode as before, `readdir` order is kept, and a
mount that does not fit the caller's buffer is still counted.

## Two fixes

- **Invalid UTF-8 broke the whole disk list.** Strings went into the JSON byte for byte. QuickJS
  refuses a byte that is not UTF-8 inside a string (`SyntaxError: invalid UTF-8 sequence`), so
  `lord.disks.list()` threw and every disk disappeared from the Disks app and the Files sidebar.
  The trigger is ordinary: a FAT label in a legacy code page (a stick formatted by Windows with an
  accented name) reaches us as `\xE9` in udev's `ID_FS_LABEL_ENC`, which `unescape_udev` turned back
  into a lone `0xE9`. A byte that is not valid UTF-8 now becomes U+FFFD, one per byte, the convention
  of `jb_str` in the rest of the code. Valid text, escapes and control characters are untouched.
- **A mount on `dm-N` never reached the disks under it.** The system-disk detection follows
  device-mapper and md stacks through `/sys/class/block/<n>/slaves` (the header says "a dm/md on top
  of it"), but it filtered names with `disk_valid_name`, which refuses the hyphen of `dm-0`. With
  LUKS or LVM the root is `dm-0`, so the disks under it were not recognized as system disks. `dm-<N>`
  is now followed; `disk_valid_name` itself is unchanged, because the actions must keep refusing it.

## Tests

- `cargo test -p axia-codecs -- disk:: smartctl::`: 20 tests on synthetic trees built in a temporary
  directory (the parsers take a root, so no host state is needed): `fgets` pieces, `sscanf` limits,
  udev properties and labels, mountinfo lines, the mount-count quirk, system-disk detection through
  dm and swap, partitions and removability, the list, the 128-partition limit, the two fixes, the
  watch lifecycle. The UTF-8 length function is compared with the standard library over every one-
  and two-byte prefix and sampled three- and four-byte ones.
- `build/disk-test` (`tests/disk_test.c`, in `make test`): the C ABI against this machine's sysfs
  and mountinfo. It asserts relations that hold anywhere, so it also passes on a build container
  with no disks.
- `build/smartctl-test` is unchanged and now links the Rust helpers.

## Validation (x86_64, 2026-11-16)

Before the C files were removed they were compared with the Rust code, symbol by symbol (the old
code compiled with its symbols renamed). Their paths could be redirected to a generated tree, and
the two fixes were applied to the reference copy so the comparison stayed like for like.

- This machine's real sysfs, udev database and mountinfo: every function, 11,226 checks, identical.
- 800 generated trees (disks, partitions, dm stacks, hostile udev and mountinfo content, lines of
  1,022 to 1,024 bytes around the `fgets` boundary, `\x00` and bad escapes, `0:N` btrfs sources,
  swaps): 3.37 million checks, identical. The same comparison against a debug build of the Rust code
  (overflow and bounds checks on): 1.7 million checks, identical, no panic.
- The comparison itself was tested by planting 26 defects in the Rust code (caps, a skipped `:`,
  token limits, sort order, escapes, `f_bavail`, the UTF-8 table, the `dm-N` filter…). The comparison
  caught 22. The other four are boundary cases of the UTF-8 table (a surrogate, an overlong form,
  above U+10FFFF, 0xC0), which the generated trees do not emit; the unit test that compares the
  function with the standard library caught all four.
- End to end through QuickJS, with the same tree (one FAT stick labelled `CAF\xC9`): the original C
  makes `lord.disks.list()` throw `SyntaxError: invalid UTF-8 sequence`; the Rust code returns the
  list, with the label `CAF` and U+FFFD.
- `libaxia_shell.a`, the archive `axia-server` links, exports the 14 `disk_*` and 3 `smartctl_*`
  functions and no test hook; `build/disk-test` (70 checks on this machine's disk) and
  `build/smartctl-test` (48 checks) pass linked against it.
- Differences found and accepted: after the string, `disk_identity` leaves the blanks it trimmed
  as zeros in the caller's buffer in C and untouched bytes in Rust. Nothing reads past the NUL.

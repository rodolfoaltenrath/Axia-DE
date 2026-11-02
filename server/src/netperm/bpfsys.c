/* bpfsys.c — direct bpf() syscall (see bpfsys.h). */
#include "netperm/bpfsys.h"

#include <errno.h>
#include <fcntl.h>
#include <linux/bpf.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>

#define ENOTSUPP_FALLBACK 524      /* the kernel's internal ENOTSUPP, which BPF calls may return */

static int sys_bpf(int cmd, union bpf_attr *a) {
    return (int)syscall(__NR_bpf, cmd, a, sizeof *a);
}

static void set_name(char *dst, const char *name) {
    memset(dst, 0, BPF_OBJ_NAME_LEN);
    if (!name) return;
    for (size_t i = 0; name[i] && i < BPF_OBJ_NAME_LEN - 1; i++)
        dst[i] = (name[i] >= 'a' && name[i] <= 'z') || (name[i] >= '0' && name[i] <= '9') || name[i] == '_' ? name[i] : '_';
}

int bpfs_map_create(uint32_t type, uint32_t key, uint32_t val, uint32_t max, uint32_t flags, const char *name) {
    union bpf_attr a;
    memset(&a, 0, sizeof a);
    a.map_type = type;
    a.key_size = key;
    a.value_size = val;
    a.max_entries = max;
    a.map_flags = flags;
    set_name(a.map_name, name);
    int fd = sys_bpf(BPF_MAP_CREATE, &a);
    if (fd < 0 && errno == EINVAL) {                /* kernels without map names */
        memset(a.map_name, 0, sizeof a.map_name);
        fd = sys_bpf(BPF_MAP_CREATE, &a);
    }
    if (fd >= 0) fcntl(fd, F_SETFD, FD_CLOEXEC);
    return fd;
}

int bpfs_map_update(int fd, const void *key, const void *val, uint64_t flags) {
    union bpf_attr a;
    memset(&a, 0, sizeof a);
    a.map_fd = (uint32_t)fd;
    a.key = (uint64_t)(uintptr_t)key;
    a.value = (uint64_t)(uintptr_t)val;
    a.flags = flags;
    return sys_bpf(BPF_MAP_UPDATE_ELEM, &a);
}

int bpfs_map_lookup(int fd, const void *key, void *val) {
    union bpf_attr a;
    memset(&a, 0, sizeof a);
    a.map_fd = (uint32_t)fd;
    a.key = (uint64_t)(uintptr_t)key;
    a.value = (uint64_t)(uintptr_t)val;
    return sys_bpf(BPF_MAP_LOOKUP_ELEM, &a);
}

int bpfs_map_delete(int fd, const void *key) {
    union bpf_attr a;
    memset(&a, 0, sizeof a);
    a.map_fd = (uint32_t)fd;
    a.key = (uint64_t)(uintptr_t)key;
    return sys_bpf(BPF_MAP_DELETE_ELEM, &a);
}

int bpfs_map_dump(int fd, void *keys, uint32_t ksz, void *vals, uint32_t vsz, uint32_t max) {
    if (!max) return 0;
    union bpf_attr a;
    uint32_t got = 0;
    uint64_t batch = 0;                 /* the kernel's cursor (u32 for hash maps; 8 bytes is enough) */
    int first = 1, batch_ok = 1;
    while (batch_ok && got < max) {
        memset(&a, 0, sizeof a);
        a.batch.map_fd = (uint32_t)fd;
        a.batch.in_batch = first ? 0 : (uint64_t)(uintptr_t)&batch;
        a.batch.out_batch = (uint64_t)(uintptr_t)&batch;
        a.batch.keys = (uint64_t)(uintptr_t)((unsigned char *)keys + (size_t)got * ksz);
        a.batch.values = (uint64_t)(uintptr_t)((unsigned char *)vals + (size_t)got * vsz);
        a.batch.count = max - got;
        int r = sys_bpf(BPF_MAP_LOOKUP_BATCH, &a);
        if (r < 0 && errno != ENOENT) {
            if (first && (errno == EINVAL || errno == ENOTSUPP_FALLBACK || errno == EOPNOTSUPP)) { batch_ok = 0; break; }
            return got ? (int)got : -1;
        }
        got += a.batch.count;
        first = 0;
        if (r < 0) return (int)got;     /* ENOENT: that was the last batch */
    }
    if (batch_ok) return (int)got;
    /* without batches: key by key */
    unsigned char cur[64], next[64];
    if (ksz > sizeof cur) { errno = EINVAL; return -1; }
    const void *prev = NULL;
    while (got < max) {
        memset(&a, 0, sizeof a);
        a.map_fd = (uint32_t)fd;
        a.key = (uint64_t)(uintptr_t)prev;
        a.next_key = (uint64_t)(uintptr_t)next;
        if (sys_bpf(BPF_MAP_GET_NEXT_KEY, &a)) break;
        memcpy(cur, next, ksz);
        prev = cur;
        if (bpfs_map_lookup(fd, cur, (unsigned char *)vals + (size_t)got * vsz)) continue;
        memcpy((unsigned char *)keys + (size_t)got * ksz, cur, ksz);
        got++;
    }
    return (int)got;
}

int bpfs_prog_load(uint32_t type, uint32_t attach, const NpProgDesc *d, int (*map_fd)(const char *), char *log, size_t loglen) {
    return bpfs_prog_load_btf(type, attach, 0, "LordOS", d, map_fd, log, loglen);
}

int bpfs_prog_load_btf(uint32_t type, uint32_t attach, uint32_t btf_id, const char *license, const NpProgDesc *d,
                       int (*map_fd)(const char *), char *log, size_t loglen) {
    if (!d || !d->ninsns || d->ninsns > 1000000) { errno = EINVAL; return -1; }
    struct bpf_insn *ins = malloc(d->ninsns * sizeof *ins);
    if (!ins) return -1;
    memcpy(ins, d->insns, d->ninsns * sizeof *ins);
    for (uint32_t i = 0; i < d->nrelocs; i++) {
        uint32_t k = d->relocs[i].insn;
        int fd = map_fd(d->relocs[i].map);
        if (k + 1 >= d->ninsns || ins[k].code != (BPF_LD | BPF_IMM | BPF_DW) || fd < 0) { free(ins); errno = EINVAL; return -1; }
        ins[k].src_reg = BPF_PSEUDO_MAP_FD;
        ins[k].imm = fd;
        ins[k + 1].imm = 0;
    }
    union bpf_attr a;
    memset(&a, 0, sizeof a);
    a.prog_type = type;
    a.expected_attach_type = attach;
    a.insns = (uint64_t)(uintptr_t)ins;
    a.insn_cnt = d->ninsns;
    a.license = (uint64_t)(uintptr_t)license;
    a.attach_btf_id = btf_id;
    set_name(a.prog_name, d->name);
    int fd = sys_bpf(BPF_PROG_LOAD, &a);
    if (fd < 0 && log && loglen > 1) {
        int e = errno;
        memset(log, 0, loglen);
        a.log_buf = (uint64_t)(uintptr_t)log;
        a.log_size = (uint32_t)(loglen > (16u << 20) ? (16u << 20) : loglen);
        a.log_level = 1;
        fd = sys_bpf(BPF_PROG_LOAD, &a);
        if (fd < 0) errno = e;
    }
    free(ins);
    if (fd >= 0) fcntl(fd, F_SETFD, FD_CLOEXEC);
    return fd;
}

int bpfs_link_create(int prog_fd, int target_fd, uint32_t attach) {
    union bpf_attr a;
    memset(&a, 0, sizeof a);
    a.link_create.prog_fd = (uint32_t)prog_fd;
    a.link_create.target_fd = (uint32_t)target_fd;
    a.link_create.attach_type = attach;
    int fd = sys_bpf(BPF_LINK_CREATE, &a);
    if (fd >= 0) fcntl(fd, F_SETFD, FD_CLOEXEC);
    return fd;
}

int bpfs_ring_open(BpfRing *r, int map_fd, size_t size) {
    memset(r, 0, sizeof *r);
    r->fd = map_fd;
    r->size = size;
    r->page = (size_t)sysconf(_SC_PAGESIZE);
    r->cons = mmap(NULL, r->page, PROT_READ | PROT_WRITE, MAP_SHARED, map_fd, 0);
    if (r->cons == MAP_FAILED) { r->cons = NULL; return -1; }
    r->prod = mmap(NULL, r->page + 2 * size, PROT_READ, MAP_SHARED, map_fd, (off_t)r->page);
    if (r->prod == MAP_FAILED) { munmap(r->cons, r->page); r->cons = r->prod = NULL; return -1; }
    return 0;
}

int bpfs_ring_drain(BpfRing *r, void (*cb)(void *u, const void *data, uint32_t len), void *u) {
    if (!r->cons) return 0;
    unsigned long *consp = r->cons, *prodp = r->prod;
    const uint8_t *data = (const uint8_t *)r->prod + r->page;
    unsigned long cons = __atomic_load_n(consp, __ATOMIC_ACQUIRE);
    int n = 0;
    for (;;) {
        unsigned long prod = __atomic_load_n(prodp, __ATOMIC_ACQUIRE);
        if (cons >= prod) break;
        const uint32_t *hdr = (const uint32_t *)(data + (cons & (r->size - 1)));
        uint32_t len = __atomic_load_n(hdr, __ATOMIC_ACQUIRE);
        if (len & BPF_RINGBUF_BUSY_BIT) break;
        uint32_t body = len & ~(uint32_t)(BPF_RINGBUF_BUSY_BIT | BPF_RINGBUF_DISCARD_BIT);
        if (!(len & BPF_RINGBUF_DISCARD_BIT) && body <= r->size) { cb(u, hdr + 2, body); n++; }
        cons += (body + BPF_RINGBUF_HDR_SZ + 7) & ~7ul;
        __atomic_store_n(consp, cons, __ATOMIC_RELEASE);
    }
    return n;
}

void bpfs_ring_close(BpfRing *r) {
    if (r->cons) munmap(r->cons, r->page);
    if (r->prod) munmap(r->prod, r->page + 2 * r->size);
    r->cons = r->prod = NULL;
}

/* ---- kernel BTF: only enough to find a function by name ---- */

static uint32_t rd32(const unsigned char *p) { uint32_t v; memcpy(&v, p, 4); return v; }

uint32_t bpfs_btf_func_id(const char *path, const char *name) {
    int fd = open(path ? path : "/sys/kernel/btf/vmlinux", O_RDONLY | O_CLOEXEC);
    if (fd < 0) return 0;
    /* sysfs reports size 0 for this file: read until EOF, up to 64 MB */
    size_t cap = 8u << 20, len = 0;
    unsigned char *buf = malloc(cap);
    while (buf) {
        if (len == cap) {
            if (cap >= (64u << 20)) { free(buf); buf = NULL; break; }
            unsigned char *nb = realloc(buf, cap * 2);
            if (!nb) { free(buf); buf = NULL; break; }
            buf = nb; cap *= 2;
        }
        ssize_t n = read(fd, buf + len, cap - len);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) break;
        len += (size_t)n;
    }
    close(fd);
    if (!buf) return 0;
    uint32_t found = 0;
    if (len < 24 || buf[0] != 0x9f || buf[1] != 0xeb) goto out;    /* magic 0xeB9F, little endian */
    uint32_t hl = rd32(buf + 4), toff = rd32(buf + 8), tlen = rd32(buf + 12), soff = rd32(buf + 16), slen = rd32(buf + 20);
    if (hl < 24 || hl > len || toff > len - hl || tlen > len - hl - toff || soff > len - hl || slen > len - hl - soff) goto out;
    const unsigned char *t = buf + hl + toff, *strs = buf + hl + soff;
    size_t nl = strlen(name), pos = 0;
    for (uint32_t id = 1; pos + 12 <= tlen; id++) {
        uint32_t noff = rd32(t + pos), info = rd32(t + pos + 4);
        uint32_t kind = (info >> 24) & 0x1f, vlen = info & 0xffff;
        size_t extra;
        switch (kind) {
        case 1: case 14: case 17: extra = 4; break;                 /* INT, VAR, DECL_TAG */
        case 3: extra = 12; break;                                  /* ARRAY */
        case 4: case 5: case 15: case 19: extra = 12u * vlen; break;/* STRUCT, UNION, DATASEC, ENUM64 */
        case 6: case 13: extra = 8u * vlen; break;                  /* ENUM, FUNC_PROTO */
        case 2: case 7: case 8: case 9: case 10: case 11: case 12: case 16: case 18: extra = 0; break;
        default: goto out;                                          /* a kind we don't know: give up */
        }
        if (kind == 12 && noff < slen && slen - noff > nl && !memcmp(strs + noff, name, nl + 1)) { found = id; break; }
        pos += 12 + extra;
    }
out:
    free(buf);
    return found;
}

uint64_t bpfs_cgroup_id(int dirfd, const char *name) {
    _Alignas(8) unsigned char buf[sizeof(struct file_handle) + sizeof(uint64_t)];
    struct file_handle *fh = (struct file_handle *)(void *)buf;
    int mount_id;
    memset(buf, 0, sizeof buf);
    fh->handle_bytes = sizeof(uint64_t);
    if (name_to_handle_at(dirfd, name, fh, &mount_id, 0) == 0 && fh->handle_bytes == sizeof(uint64_t)) {
        uint64_t id;
        memcpy(&id, fh->f_handle, sizeof id);
        return id;
    }
    struct stat st;                                  /* fallback: on 64 bits the id is the inode */
    if (fstatat(dirfd, name, &st, AT_SYMLINK_NOFOLLOW) == 0 && S_ISDIR(st.st_mode)) return (uint64_t)st.st_ino;
    return 0;
}

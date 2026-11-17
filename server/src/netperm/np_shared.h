/* np_shared.h — what axia-netperm's BPF programs (kernel.rs), the compiler of rules into maps
 * (compile.rs) and its test (tests/npcompile_test.c) share. See docs/FIREWALL.md, step 2.
 *
 * ABI declarations for C clients and regression fixtures. The shared Rust
 * implementation is compiled for both userspace and the kernel; no inline C
 * implementation remains here.
 *
 * Maps (created by axia-netperm; the programs see them by name):
 *   np_cfg     ARRAY  [0] → NpCfg                   level of the apps' scope, enabled
 *   np_apps    HASH   cgroup id (u64) → NpApp       restrictions, trusted, mode, generic list
 *   np_union   HASH   id of user@<uid> (u64) → NpApp  strict union of the user's restricted apps
 *   np_lpm4    LPM    (app idx, IPv4/n) → NpList    network and domain rules, ordered
 *   np_lpm6    LPM    (app idx, IPv6/n) → NpList
 *   np_seen    LRU    NpSeenKey → u64 ns            to report only new connections
 *   np_counts  HASH   cgroup id → NpCount           allowed / denied, bytes received / sent
 *   np_flows   LRU    NpFlowKey → NpFlow            bytes received / sent / blocked per (app, remote address)
 *   np_bound   LRU    NpBoundKey → u8               local ports an app bound (bind with port != 0): a packet
 *                                                     on one of them belongs to an incoming connection
 *   np_events  RINGBUF                              NpEvent towards axia-netperm
 *   np_snd     HASH   ALSA minor (u32) → u32           every ALSA node in /sys/class/sound: NP_SND_CAPTURE for the
 *                                                     capture PCMs (/dev/snd/pcmC*D*c), NP_SND_OTHER for the rest
 *   np_blk4    LPM    (list set, IPv4/n) → u8       (0.8.5) a user's subscribed blocklists: their networks and
 *   np_blk6    LPM    (list set, IPv6/n) → u8         the addresses the resolver gave for their domains (NpApp.blk)
 *   np_sysc    HASH   cgroup id → NpCount            (0.8.5) the essential system services' bytes (name
 *                                                     resolution, time, the network service), counted only
 *
 * Besides the network, the same per-app policy carries the device restrictions (docs/FIREWALL.md,
 * "Linux apps"): "no camera", "no USB" and "no screen capture" (/dev/fb*) (cgroup/dev, by device
 * major) and "no Bluetooth"
 * (socket(AF_BLUETOOTH), cgroup/lsm_socket_create).
 */
#ifndef LORD_NP_SHARED_H
#define LORD_NP_SHARED_H

#include <stdint.h>
typedef uint8_t np_u8;
typedef uint16_t np_u16;
typedef uint32_t np_u32;
typedef uint64_t np_u64;

#define NP_LIST_MAX   16        /* rules per address or network (including those of the networks that contain it) */
#define NP_GEN_MAX    16        /* an app's lan / internet / any rules */

#define NP_MAX_APPS   8192      /* cgroups with a policy */
#define NP_MAX_LPM    65536     /* entries of each trie */
#define NP_MAX_SEEN   16384
#define NP_MAX_BOUND  4096      /* (app, local port) of the apps' servers */
#define NP_MAX_FLOWS  8192      /* (app, remote address, port) byte counters; the least recently used go first */
#define NP_MAX_BLK    131072    /* entries of each blocklist trie (every user's sets together) */
#define NP_MAX_SYS    16        /* system service cgroups counted (np_sysc) */
#define NP_RINGBUF    (256u << 10)
#define NP_REPEAT_NS  (5ull * 1000000000ull)   /* a repeated connection is reported again after 5 s */

/* restrictions (the same bits as FW_R_* in fw/rules.h) */
#define NP_R_NO_INTERNET  (1u << 0)
#define NP_R_NO_LAN       (1u << 1)
#define NP_R_NO_NETWORK   (1u << 2)
#define NP_R_NO_INCOMING  (1u << 3)
#define NP_R_NO_CAMERA    (1u << 4)
#define NP_R_NO_MIC       (1u << 5)
#define NP_R_NO_SCREEN    (1u << 6)
#define NP_R_NO_USB       (1u << 7)
#define NP_R_NO_BLUETOOTH (1u << 8)
#define NP_R_NET_MASK     (NP_R_NO_INTERNET | NP_R_NO_LAN | NP_R_NO_NETWORK | NP_R_NO_INCOMING)
#define NP_R_DEV_MASK     (NP_R_NO_CAMERA | NP_R_NO_MIC | NP_R_NO_SCREEN | NP_R_NO_USB | NP_R_NO_BLUETOOTH)

/* Character device majors (Documentation/admin-guide/devices.txt; hidraw is dynamic and comes
 * from /proc/devices, in NpCfg). */
#define NP_MAJOR_FB       29    /* /dev/fb* (the framebuffer device) */
#define NP_MAJOR_SND      116   /* ALSA: its capture nodes (np_snd) */
#define NP_SND_OTHER      1u
#define NP_SND_CAPTURE    2u
#define NP_MAJOR_V4L      81    /* /dev/video*, /dev/v4l-subdev*, /dev/radio*, /dev/vbi* */
#define NP_MAJOR_TTYACM   166   /* USB modems and boards (/dev/ttyACM*) */
#define NP_MAJOR_TTYUSB   188   /* USB serial adapters (/dev/ttyUSB*) */
#define NP_MAJOR_USBDEV   189   /* raw USB devices (/dev/bus/usb/BBB/DDD, libusb) */
#define NP_DEV_CHAR       2     /* BPF_DEVCG_DEV_CHAR */

enum { NP_P_ANY = 0, NP_P_TCP = 1, NP_P_UDP = 2, NP_P_OTHER = 3 };   /* like FwProto (+ other) */
enum { NP_D_OUT = 0, NP_D_IN = 1, NP_D_DEV = 2, NP_D_BT = 3, NP_D_CUT = 4 };   /* events: DEV = a device open, BT = a Bluetooth socket; CUT: np_seen key of a cut socket */
enum { NP_V_ALLOW = 0, NP_V_DENY = 1 };
enum { NP_K_RULE = 0, NP_K_ANY = 1, NP_K_LAN = 2, NP_K_INET = 3 };   /* NpEnt.kind */
enum { NP_M_SILENT = 0, NP_M_OFF = 2, NP_M_DENY = 3 };               /* "ask" is treated as silent */
enum { NP_BY_RESTRICTED = 0, NP_BY_RULE = 1, NP_BY_TRUSTED = 2, NP_BY_MODE = 3, NP_BY_LIST = 4 };

typedef struct {
    np_u16 port_lo, port_hi;    /* 0..65535 = any */
    np_u8  proto;               /* NP_P_ANY / TCP / UDP */
    np_u8  dir;                 /* NP_D_OUT / NP_D_IN */
    np_u8  verdict;             /* NP_V_ALLOW / NP_V_DENY */
    np_u8  kind;                /* in the generic list: NP_K_ANY / LAN / INET; in the trie: NP_K_RULE */
    np_u32 rule_id;
} NpEnt;

typedef struct {
    np_u32 n;
    np_u32 pad;
    NpEnt  e[NP_LIST_MAX];      /* in order of precedence: the first that matches wins */
} NpList;

typedef struct {
    np_u32 idx;                 /* the app's index in the tries (per user and app) */
    np_u32 flags;               /* NP_R_* */
    np_u8  trusted;
    np_u8  mode;                /* NP_M_* */
    np_u8  has4, has6;          /* there are entries in np_lpm4 / np_lpm6 */
    np_u32 ngen;
    np_u32 can_deny;            /* 1: a restriction, a deny entry (gen or tries), the deny mode or a
                                 * blocklist set exists: the packet filter (cgroup_skb) only decides
                                 * for these apps */
    np_u32 blk;                 /* the user's blocklist set in np_blk4/np_blk6 (0: none) */
    np_u32 pad0;
    NpEnt  gen[NP_GEN_MAX];     /* lan / internet / any rules, in order */
} NpApp;

typedef struct { np_u32 prefixlen; np_u8 data[8]; } NpLpm4Key;    /* idx (big endian) + IPv4 */
typedef struct { np_u32 prefixlen; np_u8 data[20]; } NpLpm6Key;   /* idx (big endian) + IPv6 */

typedef struct {
    np_u32 level;               /* depth of the apps' scopes (bpf_get_current_ancestor_cgroup_id) */
    np_u32 on;                  /* 0: everything is allowed without looking at anything */
    np_u32 user_level;          /* depth of user@<uid>.service; 0: don't look at the user@ union */
    np_u32 hidraw_major;        /* /dev/hidraw* (dynamic major), 0 if the kernel has none */
} NpCfg;

typedef struct {
    np_u64 cgid;
    np_u8  addr[16];
    np_u16 port;
    np_u8  fam, proto, dir, verdict;
    np_u8  pad[2];
} NpSeenKey;

/* allowed / denied: connections (connect, bind, the first sendto in 5 s, and each open socket
 * cut because a rule now denies it). rx / tx: IP bytes received and sent by the app's sockets
 * (cgroup/skb_ingress and skb_egress, every protocol: TCP, UDP and so QUIC), loopback excluded so
 * they compare with the interfaces' counters. blocked: IP bytes of packets the same programs
 * dropped (in and out) because the app's current rules deny their connection; not in rx / tx. */
typedef struct { np_u64 allowed, denied, rx, tx, blocked; } NpCount;

typedef struct {
    np_u64 cgid;
    np_u8  addr[16];            /* the remote address; IPv4 (also IPv4-mapped IPv6) in the first 4 */
    np_u8  fam;                 /* 4 or 6 */
    np_u8  proto;               /* (0.8.5) NP_P_TCP / NP_P_UDP / NP_P_OTHER */
    np_u8  dir;                 /* (0.8.5) NP_D_OUT: port is the remote one; NP_D_IN: a port the app
                                 * bound (np_bound), its local one */
    np_u8  pad;
    np_u16 port;                /* (0.8.5) host order; 0 when there is none (ICMP, a later fragment) */
    np_u8  pad2[2];
} NpFlowKey;

typedef struct { np_u64 rx, tx, blocked; } NpFlow;

typedef struct {
    np_u64 cgid;                /* the app's scope */
    np_u16 port;                /* local port, host order */
    np_u8  proto;               /* NP_P_TCP / NP_P_UDP */
    np_u8  pad[5];
} NpBoundKey;

typedef struct {
    np_u64 cgid;
    np_u64 ts;                  /* ns from bpf_ktime_get_ns */
    np_u8  addr[16];            /* IPv4 in the first 4 bytes */
    np_u16 port;
    np_u8  fam, proto, dir, verdict, reason, record;
    np_u32 rule_id;
    np_u32 flag;
    /* only for a cut (an open socket whose packets the filter now drops): */
    np_u8  laddr[16];           /* the local address (same family as addr) */
    np_u16 lport, rport;        /* the local and remote ports (port is the one the rule looked at) */
    np_u8  cut;                 /* 0: a connection attempt; 1: first drop of this socket; 2: repeated */
    np_u8  destroy;             /* 1: a TCP connection (not a listener) or a connected UDP socket:
                                 * axia-netperm destroys it. 0: an unconnected UDP socket or a
                                 * listener, which may also serve allowed peers: only the drop */
    np_u8  pad2[2];
    np_u64 cookie;              /* the socket's cookie (bpf_get_socket_cookie), checked by SOCK_DESTROY */
} NpEvent;

typedef struct {
    int    verdict, reason, record;
    np_u32 rule_id, flag;
} NpRes;

/* Compatibility declarations only. All implementations, including the kernel's
 * decision function, live in shared.rs. */
np_u32 np_dev_restriction(np_u32 type, np_u32 major, np_u32 hidraw_major);
int np_is_lan4(const np_u8 *a);
int np_is_lan6(const np_u8 *a);
int np_is_loop(int fam, const np_u8 *a);
int np_ent_match(const NpEnt *e, int dir, int proto, int port);
void np_decide(const NpApp *a, const NpList *l, int dir, int proto, int port,
               int lan, int listed, int loop, NpRes *r);
int np_ents_deny(const NpEnt *e, np_u32 n);
#endif

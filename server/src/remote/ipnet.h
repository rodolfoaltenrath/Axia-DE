/* ipnet.h — addresses, networks (CIDR) and the remote desktop's address rules (docs/REMOTE.md,
 * "Security analysis (0.8.3)"). Plain libc: shared by lord-sessiond (who may connect) and the
 * admin helper (the remote-block / remote-unblock / remote-allow actions validate with it).
 *
 * An IPv4 address is kept as its IPv4-mapped IPv6 form (::ffff:a.b.c.d), so one comparison covers
 * both families; prefixes of IPv4 networks are stored +96. */
#ifndef LORD_REMOTE_IPNET_H
#define LORD_REMOTE_IPNET_H

#include <stddef.h>

typedef struct { unsigned char a[16]; int v4; } RsAddr;
typedef struct { RsAddr net; int prefix; } RsCidr;          /* prefix 0..128 over the mapped form */

/* Shortest networks the actions accept (a block of 0.0.0.0/0 would shut out everybody). */
#define RS_MIN_PREFIX4 8
#define RS_MIN_PREFIX6 16

#define RS_ACCESS_MAX 64           /* manual entries of each kind */

/* Strict textual forms: dotted quad or RFC 4291 text (no zone, no spaces). IPv4-mapped IPv6 is
 * taken as IPv4. 0 on success. */
int  rs_addr_parse(const char *s, RsAddr *a);
/* "addr" or "addr/len" (decimal, no leading zeros, v4 8..32, v6 16..128). Host bits are cleared. */
int  rs_cidr_parse(const char *s, RsCidr *c);
int  rs_cidr_contains(const RsCidr *c, const RsAddr *a);
/* is network `inner` entirely inside `outer`? */
int  rs_cidr_covers(const RsCidr *outer, const RsCidr *inner);
int  rs_cidr_equal(const RsCidr *x, const RsCidr *y);
void rs_addr_format(const RsAddr *a, char *out, size_t n);
void rs_cidr_format(const RsCidr *c, char *out, size_t n);  /* "a.b.c.d/24", "fd00::/8", single hosts without /len */
/* Lockout key of an address: IPv4 as is, IPv6 as its /64 ("2001:db8:1:2::/64"). */
void rs_addr_key(const RsAddr *a, char *out, size_t n);
int  rs_key_parse(const char *key, RsCidr *c);              /* the reverse; 0 if it's a valid key */
/* Private, loopback, link-local, CGNAT and unique-local ranges (never public internet). */
int  rs_addr_is_local(const RsAddr *a);

/* Manual lists (/etc/lordos/remote/access, written only by the admin actions):
 *   allow <cidr>      networks for net=list
 *   block <cidr>      blocked until removed */
typedef struct { RsCidr allow[RS_ACCESS_MAX]; int nallow; RsCidr block[RS_ACCESS_MAX]; int nblock; } RsAccess;
/* Unknown or invalid lines are skipped (counted in *bad if not NULL). */
void rs_access_parse(const char *text, RsAccess *acc, int *bad);
/* Text for the file; returns the length or -1 if it doesn't fit. */
int  rs_access_format(const RsAccess *acc, char *out, size_t n);
/* 0 added, 1 already there, -1 full */
int  rs_access_add(RsCidr *list, int *n, const RsCidr *c);
/* removes entries equal to c or inside it; returns how many */
int  rs_access_del(RsCidr *list, int *n, const RsCidr *c);

enum { RS_NET_LAN = 0, RS_NET_LIST = 1, RS_NET_ANY = 2 };   /* config net=lan|list|any */
int  rs_net_mode(const char *s);                            /* -1 if invalid */
const char *rs_net_name(int mode);

enum { RS_ACC_OK = 0, RS_ACC_BLOCKED = 1, RS_ACC_NOT_ALLOWED = 2 };
/* Who may open a connection: a manual block always wins; then net=any lets everybody in, net=list
 * only the listed networks, net=lan (the default) local ranges or a connection that arrived on a
 * VPN interface (via_vpn). */
int  rs_access_decide(const RsAccess *acc, int mode, const RsAddr *peer, int via_vpn);
/* Interface names LordOS's VPN manager creates: wg-, ovpn-, ips-, ppp-, oc-, fvpn- + a name. */
int  rs_vpn_ifname(const char *name);
/* Does a connection count as "over a LordOS VPN" for net=lan? Only if the interface is one of those,
 * the connection's local address is that interface's address, and the peer is inside the interface's
 * own subnet (prefix over the mapped form; at least /8 for IPv4, /16 for IPv6) or is its
 * point-to-point peer (ptp, may be NULL). */
int  rs_vpn_accepts(const char *ifname, const RsAddr *ifaddr, int prefix, const RsAddr *ptp, const RsAddr *local, const RsAddr *peer);

#endif

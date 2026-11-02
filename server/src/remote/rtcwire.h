/* rtcwire.h — the WebRTC wire formats of the remote desktop's video path (docs/REMOTE.md, "Video
 * (WebRTC)"): the browser's SDP offer and our answer, ICE candidates, STUN and TURN messages, RTCP
 * feedback, the data channel's open message, and the RTP payload formats we send (H.264, VP8,
 * Opus). Plain libc, no I/O and no OpenSSL, like rsec.c: unit-tested and fuzzed under ASan+UBSan
 * (build/remote-sec-test, build/remote-fuzz).
 *
 * Every parser rejects instead of guessing. The browser's SDP is only accepted over the remote
 * desktop's own authenticated WebSocket; everything else here arrives on the bridge's UDP socket, an
 * ICE-TCP connection or the TURN server's stream, and is parsed before any cryptography looks at it. */
#ifndef LORD_REMOTE_RTCWIRE_H
#define LORD_REMOTE_RTCWIRE_H

#include <stddef.h>
#include <stdint.h>

/* ---- addresses (IPv4 or IPv6, network order bytes) ---- */

typedef struct { int family; uint8_t a[16]; uint16_t port; } RwAddr;     /* family 4 or 6, 0 = none */
int  rw_addr_text(const char *s, RwAddr *a);           /* dotted quad or RFC 4291 text; port untouched */
void rw_addr_format(const RwAddr *a, char *out, size_t n);
int  rw_addr_equal(const RwAddr *x, const RwAddr *y);  /* address and port */

/* Settings → Sharing's ICE servers (validated by the admin helper and again by lord-sessiond):
 * "host:port" with a DNS name ([A-Za-z0-9.-], 1..63), an IPv4 or an [IPv6], port 1..65535. */
int  rw_ice_server_valid(const char *s);
/* A TURN user name: 1..128 printable ASCII, no space, quote or backslash. Password: 1..128 printable ASCII. */
int  rw_turn_user_valid(const char *s);
int  rw_turn_pass_valid(const char *s);
/* How the bridge reaches its TURN server: "udp" (the default), "tcp" or "tls" (TURN over TLS, the
 * turns: scheme; port 443 works). RW_TURN_* or -1. */
enum { RW_TURN_UDP = 0, RW_TURN_TCP = 1, RW_TURN_TLS = 2 };
int  rw_turn_transport(const char *s);
const char *rw_turn_transport_name(int t);
/* A pinned SHA-256 certificate fingerprint for a TURN-over-TLS server with its own certificate:
 * 64 hex digits, optionally in pairs separated by ':' (upper or lower case). 0 and out[32], or -1. */
int  rw_fp_parse(const char *s, uint8_t out[32]);

/* ---- SDP: the browser's offer ---- */

#define RW_SDP_MAX 32768       /* bytes; Chrome's offer with every codec is ~7 KB */
#define RW_SDP_LINES 600
#define RW_SDP_LINE 1024
#define RW_MAX_MEDIA 4         /* video, audio, data channel (+1 spare) */
#define RW_MAX_PT 64           /* payload types per m= line */
#define RW_MAX_CAND 16

enum { RW_KIND_AUDIO = 1, RW_KIND_VIDEO, RW_KIND_APP };
enum { RW_DIR_SENDRECV = 0, RW_DIR_RECVONLY, RW_DIR_SENDONLY, RW_DIR_INACTIVE };
enum { RW_SETUP_ACTPASS = 1, RW_SETUP_ACTIVE, RW_SETUP_PASSIVE };
enum { RW_CODEC_NONE = 0, RW_CODEC_H264 = 1, RW_CODEC_VP8 = 2 };
enum { RW_CAND_HOST = 1, RW_CAND_SRFLX, RW_CAND_PRFLX, RW_CAND_RELAY };

typedef struct {
    int pt;
    char name[16];
    int clock, channels;
    int fb_nack, fb_pli, fb_fir;
    int pmode;                  /* H.264 packetization-mode */
    uint32_t plid;              /* H.264 profile-level-id (0 if absent) */
    int has_fmtp;
} RwCodec;

typedef struct {
    int kind, rejected, dir;
    char mid[33];
    int npt;
    RwCodec pt[RW_MAX_PT];
    int sctp_port;
    long max_msg;
} RwMedia;

typedef struct {
    int mdns;                   /* the address is an mDNS name (unusable here) */
    int tcp;                    /* a TCP candidate (ignored) */
    int component, type;
    uint32_t prio;
    RwAddr addr;
} RwCand;

typedef struct {
    char ufrag[257], pwd[257];
    uint8_t fp[32];             /* a=fingerprint:sha-256 (the browser's DTLS certificate) */
    int setup;
    int nmedia;
    RwMedia m[RW_MAX_MEDIA];
    int ncand;
    RwCand cand[RW_MAX_CAND];
    /* chosen by rw_sdp_parse_offer: index of each section (-1 none), payload type, codec */
    int vm, vpt, vcodec, am, apt, dm;
} RwOffer;

/* Error codes (< 0) of the SDP and candidate parsers. */
enum { RW_OK = 0, RW_ESYNTAX = -1, RW_ETOOBIG = -2, RW_EICE = -3, RW_EFP = -4, RW_EMEDIA = -5, RW_ECODEC = -6 };
/* Strict: CRLF (or LF) lines of printable ASCII, "x=" types in order, at most RW_SDP_LINES lines of
 * RW_SDP_LINE bytes; one ufrag, pwd and sha-256 fingerprint for the whole bundle (session level or the
 * same in every section); setup actpass or active; every section bundled with rtcp-mux; at most one
 * video, one audio and one data channel section. Unknown attributes are skipped (browsers send many),
 * the ones we use are checked. Then it picks the codecs: H.264 constrained baseline with
 * packetization-mode 1 if offered and allow_h264, else VP8; Opus 48 kHz stereo. */
int  rw_sdp_parse_offer(const char *sdp, size_t n, int allow_h264, RwOffer *o);

/* "candidate:..." (with or without the "a=" prefix), RFC 8839 grammar, at most 512 bytes. */
int  rw_cand_parse(const char *s, size_t n, RwCand *c);

/* ---- SDP: our answer ---- */

typedef struct {
    char ip[64];
    uint16_t port;
    int type;                   /* RW_CAND_HOST, SRFLX or RELAY */
    uint32_t prio;
    char raddr[64];
    uint16_t rport;
    int tcp;                    /* a passive ICE-TCP host candidate (RFC 6544) */
} RwLocalCand;

typedef struct {
    const char *ufrag, *pwd;    /* ours, [A-Za-z0-9+/] */
    const uint8_t *fp;          /* our certificate's SHA-256 */
    uint32_t vssrc, assrc;
    int audio;                  /* send audio (else the audio section is answered inactive) */
    const RwLocalCand *cand;
    int ncand;
    uint32_t sess_id;
    unsigned version;           /* o= session version: 2, one more for each ICE restart */
} RwAnswer;
/* ICE-lite, setup:passive, each offered section answered in order (unusable ones rejected with port 0).
 * Returns the length or -1 if it doesn't fit. */
int  rw_sdp_answer(const RwOffer *o, const RwAnswer *a, char *out, size_t cap);

/* ---- STUN (RFC 8489) and TURN (RFC 8656) ---- */

#define RW_STUN_MAX 1500
enum {
    RW_STUN_BINDING_REQ = 0x0001, RW_STUN_BINDING_OK = 0x0101, RW_STUN_BINDING_ERR = 0x0111,
    RW_STUN_BINDING_IND = 0x0011,
    RW_TURN_ALLOCATE_REQ = 0x0003, RW_TURN_ALLOCATE_OK = 0x0103, RW_TURN_ALLOCATE_ERR = 0x0113,
    RW_TURN_REFRESH_REQ = 0x0004, RW_TURN_REFRESH_OK = 0x0104, RW_TURN_REFRESH_ERR = 0x0114,
    RW_TURN_PERM_REQ = 0x0008, RW_TURN_PERM_OK = 0x0108, RW_TURN_PERM_ERR = 0x0118,
    RW_TURN_SEND_IND = 0x0016, RW_TURN_DATA_IND = 0x0017,
};
typedef struct {
    uint16_t type;
    uint8_t tid[12];
    const uint8_t *username;    /* inside the packet */
    size_t ulen;
    size_t mi_off;              /* offset of MESSAGE-INTEGRITY (0 = none) */
    const uint8_t *mi;          /* its 20 bytes */
    int has_fp;                 /* FINGERPRINT present (and right: rw_stun_parse checks it) */
    int use_candidate, controlling, controlled;
    uint32_t priority;
    RwAddr xor_mapped, xor_relayed, xor_peer;
    int error;                  /* ERROR-CODE class*100+number, 0 none */
    const uint8_t *realm, *nonce, *data;
    size_t rlen, nlen, dlen;
    uint32_t lifetime;
} RwStun;

/* ICE over TCP (RFC 6544): every packet framed by a 16-bit big-endian length (RFC 4571). Is there a
 * whole frame at the start of p? 1 (*len = its payload length, the payload at p + 2), 0 not yet, -1
 * never (a zero or oversized length: the connection is closed). */
#define RW_TCP_FRAME_MAX 4096
int  rw_tcp_frame(const uint8_t *p, size_t n, size_t *len);
/* A TURN server's TCP or TLS stream carries bare STUN messages, delimited by their own length (RFC
 * 8656 §6.2; we never bind channels, so ChannelData is refused). 1 (*len = the whole message), 0 not
 * yet, -1 never. */
int  rw_stun_stream(const uint8_t *p, size_t n, size_t *len);
/* The first ICE check on a new ICE-TCP connection, read by lord-sessiond's root connection child
 * (before any bridge is involved): a STUN binding request whose USERNAME starts with the 8 routing
 * characters [A-Za-z0-9] of the bridge it is for (our ufrag is <route><4 random>, see rtc.c). 0 and
 * route[9], or -1. Nothing else of the message is trusted here: the bridge checks its integrity. */
int  rw_ice_route(const uint8_t *p, size_t n, char route[9]);

/* First byte demultiplexing (RFC 7983): 0..3 STUN, 20..63 DTLS, 128..191 RTP/RTCP. */
int  rw_is_stun(const uint8_t *p, size_t n);
/* Strict: header (magic cookie, length a multiple of 4 and exact), attributes with their padding,
 * known ones with exact sizes, nothing after FINGERPRINT, only FINGERPRINT after MESSAGE-INTEGRITY,
 * at most one of each. 0 or -1. */
int  rw_stun_parse(const uint8_t *p, size_t n, RwStun *s);
uint32_t rw_crc32(const uint8_t *p, size_t n);

/* HMAC-SHA1 (MESSAGE-INTEGRITY) is provided by the caller (OpenSSL in rtc.c). */
typedef void (*RwHmac)(const uint8_t *key, size_t klen, const uint8_t *msg, size_t n, uint8_t out[20]);
/* Does the message's MESSAGE-INTEGRITY match this key? (constant-time compare) */
int  rw_stun_check_mi(const uint8_t *p, size_t n, const RwStun *s, const uint8_t *key, size_t klen, RwHmac hmac);

typedef struct { uint8_t b[RW_STUN_MAX]; size_t n; int bad; } RwStunOut;
void rw_so_init(RwStunOut *o, uint16_t type, const uint8_t tid[12]);
void rw_so_attr(RwStunOut *o, uint16_t type, const void *v, size_t len);
void rw_so_u32(RwStunOut *o, uint16_t type, uint32_t v);
void rw_so_xaddr(RwStunOut *o, uint16_t type, const RwAddr *a);
void rw_so_mi(RwStunOut *o, const uint8_t *key, size_t klen, RwHmac hmac);
void rw_so_fingerprint(RwStunOut *o);
enum { RW_ATTR_MAPPED = 0x0001, RW_ATTR_USERNAME = 0x0006, RW_ATTR_MI = 0x0008, RW_ATTR_ERROR = 0x0009,
       RW_ATTR_CHANNEL = 0x000c, RW_ATTR_LIFETIME = 0x000d, RW_ATTR_XOR_PEER = 0x0012, RW_ATTR_DATA = 0x0013,
       RW_ATTR_REALM = 0x0014, RW_ATTR_NONCE = 0x0015, RW_ATTR_XOR_RELAYED = 0x0016, RW_ATTR_TRANSPORT = 0x0019,
       RW_ATTR_XOR_MAPPED = 0x0020, RW_ATTR_PRIORITY = 0x0024, RW_ATTR_USE_CAND = 0x0025,
       RW_ATTR_SOFTWARE = 0x8022, RW_ATTR_FINGERPRINT = 0x8028, RW_ATTR_CONTROLLED = 0x8029, RW_ATTR_CONTROLLING = 0x802a };

/* ---- RTCP from the browser (RFC 3550, 4585, 5104; REMB) ---- */

enum { RW_FB_RR = 1, RW_FB_PLI, RW_FB_FIR, RW_FB_NACK, RW_FB_REMB };
typedef struct {
    int kind;
    uint32_t ssrc;              /* the media source the item is about */
    int lost_frac;              /* RR: fraction lost ×256 */
    uint32_t jitter, lsr, dlsr; /* RR */
    uint16_t pid, blp;          /* NACK */
    uint64_t bitrate;           /* REMB, bits/s */
} RwFb;
typedef void (*RwFbOn)(void *ud, const RwFb *f);
/* A compound packet (already decrypted): every part checked (version, lengths, padding); calls on()
 * for each report block and feedback item. 0 or -1 (nothing is reported for a malformed packet). */
int  rw_rtcp_parse(const uint8_t *p, size_t n, RwFbOn on, void *ud);

/* ---- data channel establishment (RFC 8832) ---- */

typedef struct { int chan_type; uint16_t prio; uint32_t rel; char label[65]; } RwDcOpen;
/* DATA_CHANNEL_OPEN: reliable (0x00) or reliable unordered (0x80) only, a label of 1..64 bytes of
 * [a-z0-9_-], no subprotocol. 0 or -1. */
int  rw_dcep_open(const uint8_t *p, size_t n, RwDcOpen *o);

/* ---- RTP payloads we send ---- */

/* Each RTP payload: a payload header (hl bytes, may be 0) then n bytes of data; marker on the last
 * packet of a frame. */
typedef void (*RwPktOut)(void *ud, const uint8_t *hdr, size_t hl, const uint8_t *data, size_t n, int marker);
/* An H.264 access unit in Annex B: single NAL unit packets or FU-A fragments (RFC 6184, mode 1).
 * Returns the number of packets or -1 (no NAL unit, more than 256, or mtu too small). */
int  rw_h264_packetize(const uint8_t *au, size_t n, size_t mtu, RwPktOut out, void *ud);
/* A VP8 frame (RFC 7741): the 1-byte descriptor, S on the first packet. */
int  rw_vp8_packetize(const uint8_t *f, size_t n, size_t mtu, RwPktOut out, void *ud);
/* Is an H.264 access unit an IDR (has a NAL unit of type 5)? */
int  rw_h264_is_idr(const uint8_t *au, size_t n);

#endif

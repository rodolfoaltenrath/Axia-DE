/* rtc.h — the remote desktop's WebRTC peer, inside the bridge (see rtc.c and docs/REMOTE.md,
 * "Video (WebRTC)"). One per browser connection, in the process that already dropped root: ICE-lite
 * over UDP and TCP (passive candidates on the remote desktop's own port) with a built-in STUN
 * responder, ICE restarts, DTLS-SRTP (OpenSSL), SRTP (libsrtp2), the data channels over SCTP (usrsctp),
 * RTP for the session's encoded video and Opus audio, and an optional TURN relay over UDP, TCP or TLS.
 * The WebRTC peer is the only way the remote desktop shows the screen: there is no other path. */
#ifndef LORD_REMOTE_RTC_H
#define LORD_REMOTE_RTC_H

#include <poll.h>
#include <stddef.h>
#include <stdint.h>

#include "ipnet.h"
#include "rtcwire.h"

typedef struct Rtc Rtc;

enum { RTC_CONNECTED = 1, RTC_DC_OPEN, RTC_FAILED };
enum { RTC_CH_INPUT = 0, RTC_CH_TILES = 1 };

/* Why a peer failed, as told to the browser (it explains it and says what to do; docs/REMOTE.md,
 * "When it can't connect"): */
#define RTC_WHY_NOROUTE "noroute"   /* no ICE check reached us: UDP and TCP blocked, or no route */
#define RTC_WHY_DTLS    "dtls"      /* checks arrived but DTLS never came up, or the certificate didn't match */
#define RTC_WHY_LOST    "lost"      /* it was up, then the browser stopped answering */
#define RTC_WHY_CLOSED  "closed"    /* the browser closed DTLS or the data channels */
#define RTC_WHY_SERVER  "server"    /* something on this side (memory, sockets, SCTP) */

typedef struct {
    void (*signal)(void *ud, const char *json);         /* a message for the browser over the WebSocket */
    void (*state)(void *ud, int state, const char *why);   /* RTC_FAILED: why is one of RTC_WHY_* */
    void (*message)(void *ud, int chan, const uint8_t *p, size_t n, int binary);   /* from a data channel */
    void (*keyframe)(void *ud);                         /* the browser lost a frame (PLI/FIR) */
    void (*rate)(void *ud, unsigned kbps);              /* new target bit rate for this viewer */
    void *ud;
} RtcCallbacks;

#define RTC_MAX_STUN 4
typedef struct {
    const RsAccess *access;     /* who may send us UDP: the same rules as the WebSocket (ipnet.c) */
    int net_mode;
    RwAddr ws_peer, ws_local;   /* the authenticated WebSocket's addresses */
    int nstun;
    char stun[RTC_MAX_STUN][96];        /* host:port of external STUN servers (Settings → Sharing) */
    char turn[96], turn_user[129];      /* host:port and user of a TURN server, or "" */
    int turn_transport;                 /* RW_TURN_UDP, RW_TURN_TCP or RW_TURN_TLS */
    int turn_has_fp;                    /* TLS: pin this certificate instead of the system's CAs */
    uint8_t turn_fp[32];
    char *turn_pass;            /* in memory only; rtc_new takes it over and wipes it when done */
    /* or, for a later peer of the same bridge, the long-term key the first one derived (MD5 of
     * user:realm:password, RFC 8489 §9.2.2), for that realm only; never the password again */
    int turn_has_key;
    uint8_t turn_key[16];
    char turn_realm[256];
    unsigned max_kbps;
    /* ICE over TCP: lord-sessiond's root connection child hands us, over this listening socket (root's
     * /run/lordos/rtc/<route>, created before the bridge dropped root), each TCP connection to the
     * remote desktop port whose first ICE check names our route; -1: no ICE-TCP */
    int ice_lfd;
    char route[9];                      /* 8 characters [A-Za-z0-9]: the start of every ufrag we use */
    int tcp_port;                       /* the remote desktop port, for our passive TCP candidates */
    int peer_no;                        /* this connection's peer number, echoed in our answers ("p") */
} RtcConfig;

Rtc *rtc_new(const RtcConfig *cfg, const RtcCallbacks *cb);
void rtc_free(Rtc *r);
int  rtc_port(const Rtc *r);                /* the built-in STUN server's UDP port (for the browser) */

/* The browser's offer (from the authenticated WebSocket): parsed strictly (rtcwire.c); the answer goes
 * out through signal(). Returns the chosen RW_CODEC_* or < 0 (RW_E*). A later offer is an ICE restart:
 * new ICE credentials on both sides, the same DTLS association (its fingerprint must not change), the
 * same codec and sections; anything else is refused (-1) and the browser starts a new peer. */
int  rtc_offer(Rtc *r, const char *sdp, size_t n, int allow_h264);
/* A trickled candidate from the browser ("candidate:..."): only used for TURN permissions. */
int  rtc_candidate(Rtc *r, const char *c, size_t n);

#define RTC_MAX_FDS 9                   /* UDP media, UDP STUN, the ICE-TCP handoff, 4 ICE-TCP, TURN */
int  rtc_pollfds(Rtc *r, struct pollfd *p, int max);
void rtc_dispatch(Rtc *r, const struct pollfd *p, int n);
int  rtc_timeout(Rtc *r);                   /* ms until rtc_tick is due */
void rtc_tick(Rtc *r);

int  rtc_codec(const Rtc *r);               /* RW_CODEC_* agreed, 0 before the offer */
int  rtc_media_ready(const Rtc *r);         /* SRTP keys are there: media can flow */
/* One encoded video frame (H.264 Annex B or VP8), ms on the session's clock. */
void rtc_video(Rtc *r, const uint8_t *f, size_t n, uint32_t ms);
/* One Opus packet, its first sample on a 48 kHz clock. */
void rtc_audio(Rtc *r, const uint8_t *p, size_t n, uint32_t samples);
/* Data channel message; -1 if the channel isn't open or its send buffer is full (try later). */
int  rtc_send(Rtc *r, int chan, const void *p, size_t n, int binary);
int  rtc_dc_open(const Rtc *r, int chan);
/* The TURN long-term key and realm of an allocated relay, for the bridge's next peer. 0 or -1. */
int  rtc_turn_key(const Rtc *r, uint8_t key[16], char realm[256]);
/* "udp", "tcp" or "relay": how the media goes now ("" before it does) */
const char *rtc_path_kind(const Rtc *r);
/* Line for the logs and the browser's status: "h264 2400 kbps 1.2 % lost" */
void rtc_stats(const Rtc *r, char *out, size_t n);

#endif

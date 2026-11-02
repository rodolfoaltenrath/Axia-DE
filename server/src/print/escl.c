/* escl.c — see escl.h. The scanner's XML is read with our own bounded tag finder
 * (no libxml2): only the values that matter are taken and prefixes (scan:, pwg:) are ignored. */
#include "escl.h"
#include "phttp.h"

#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define ESCL_MAX (256u << 20)

int escl_valid_rs(const char *rs) {
    size_t n = strlen(rs);
    if (n > 64 || strstr(rs, "..")) return 0;
    for (size_t i = 0; i < n; i++) if (!isalnum((unsigned char)rs[i]) && rs[i] != '/' && rs[i] != '_' && rs[i] != '-' && rs[i] != '.') return 0;
    return 1;
}

static void err(JBuf *out, const char *code, const char *detail) {
    jb_free(out);
    jb_raw(out, "{\"error\":");
    jb_str(out, code);
    jb_raw(out, ",\"detail\":");
    jb_str(out, detail ? detail : "");
    jb_raw(out, "}");
}

/* Next occurrence of <prefix:tag> or <tag> from *pos; copies the text up to '<'. */
static int next_tag(const char *xml, size_t len, size_t *pos, const char *tag, char *out, size_t n) {
    size_t tl = strlen(tag);
    for (size_t i = *pos; i + tl + 2 < len; i++) {
        if (xml[i] != '<' || xml[i + 1] == '/') continue;
        size_t j = i + 1;
        while (j < len && j < i + 40 && xml[j] != ':' && xml[j] != '>' && xml[j] != ' ') j++;
        size_t name = j < len && xml[j] == ':' ? j + 1 : i + 1;
        if (name + tl >= len || strncmp(xml + name, tag, tl) || (xml[name + tl] != '>' && xml[name + tl] != ' ')) continue;
        size_t v = name + tl;
        while (v < len && xml[v] != '>') v++;
        v++;
        size_t k = 0;
        while (v < len && xml[v] != '<' && k + 1 < n) out[k++] = xml[v++];
        out[k] = 0;
        *pos = v;
        return 1;
    }
    return 0;
}

static int get(const char *ip, int port, const char *path, PResp *r, int timeout) {
    int fd = phttp_tcp(ip, port, 4000);
    if (fd < 0) return -1;
    char host[64];
    snprintf(host, sizeof host, "%s:%d", ip, port);
    int x = phttp_request(fd, "GET", host, path, NULL, NULL, NULL, 0, -1, 0, ESCL_MAX, timeout, r);
    close(fd);
    return x;
}

int escl_caps(const char *ip, int port, const char *rs, JBuf *out) {
    if (!phttp_lan_ipv4(ip) || !escl_valid_rs(rs)) { err(out, "bad-request", "host"); return -1; }
    char path[128];
    snprintf(path, sizeof path, "/%s/ScannerCapabilities", rs);
    PResp r;
    if (get(ip, port, path, &r, 8000)) { err(out, "unreachable", ""); return -1; }
    if (r.status != 200 || !r.body) { char d[16]; snprintf(d, sizeof d, "%d", r.status); presp_free(&r); err(out, "http", d); return -1; }
    const char *x = r.body;
    size_t len = r.len;
    char v[256];
    size_t pos = 0;
    jb_free(out);
    jb_raw(out, "{\"model\":");
    jb_str(out, next_tag(x, len, &pos, "MakeAndModel", v, sizeof v) ? v : "");
    /* what's on the glass (Platen) and the feeder (Adf) */
    jb_printf(out, ",\"platen\":%s,\"adf\":%s", strstr(x, "Platen>") || strstr(x, "PlatenInputCaps") ? "true" : "false",
              strstr(x, "AdfSimplexInputCaps") || strstr(x, "Adf>") ? "true" : "false");
    pos = 0;
    long maxw = 2550, maxh = 3508;
    if (next_tag(x, len, &pos, "MaxWidth", v, sizeof v)) maxw = strtol(v, NULL, 10);
    pos = 0;
    if (next_tag(x, len, &pos, "MaxHeight", v, sizeof v)) maxh = strtol(v, NULL, 10);
    if (maxw < 100 || maxw > 20000) maxw = 2550;
    if (maxh < 100 || maxh > 40000) maxh = 3508;
    jb_printf(out, ",\"maxW\":%ld,\"maxH\":%ld", maxw, maxh);
    /* lists without duplicates */
    static const char *const lists[][2] = { { "colors", "ColorMode" }, { "formats", "DocumentFormatExt" }, { "formats2", "DocumentFormat" }, { "res", "XResolution" } };
    for (size_t k = 0; k < sizeof lists / sizeof lists[0]; k++) {
        jb_printf(out, ",\"%s\":[", lists[k][0]);
        char seen[32][64];
        int ns = 0;
        pos = 0;
        while (ns < 32 && next_tag(x, len, &pos, lists[k][1], v, sizeof v)) {
            int dup = 0;
            for (int i = 0; i < ns; i++) if (!strcmp(seen[i], v)) dup = 1;
            if (dup || !v[0] || strlen(v) >= 64) continue;
            snprintf(seen[ns], sizeof seen[0], "%s", v);
            if (ns) jb_raw(out, ",");
            if (k == 3) jb_printf(out, "%ld", strtol(v, NULL, 10));
            else jb_str(out, v);
            ns++;
        }
        jb_raw(out, "]");
    }
    jb_raw(out, "}");
    presp_free(&r);
    return 0;
}

static const char *ext_of(const char *mime) {
    if (!strncmp(mime, "image/png", 9)) return "png";
    if (!strncmp(mime, "application/pdf", 15)) return "pdf";
    if (!strncmp(mime, "image/jpeg", 10)) return "jpg";
    return NULL;
}

int escl_scan(const char *ip, int port, const char *rs, const JVal *s, const char *dir, JBuf *out) {
    if (!phttp_lan_ipv4(ip) || !escl_valid_rs(rs)) { err(out, "bad-request", "host"); return -1; }
    const char *color = json_str(s, "color"), *format = json_str(s, "format"), *source = json_str(s, "source");
    if (!color || (strcmp(color, "RGB24") && strcmp(color, "Grayscale8") && strcmp(color, "BlackAndWhite1"))) color = "RGB24";
    if (!format || !ext_of(format)) format = "image/jpeg";
    if (!source || (strcmp(source, "Platen") && strcmp(source, "Feeder"))) source = "Platen";
    int res = (int)json_num(s, "res", 300);
    if (res < 50 || res > 2400) res = 300;
    long x = (long)json_num(s, "x", 0), y = (long)json_num(s, "y", 0), w = (long)json_num(s, "w", 2550), h = (long)json_num(s, "h", 3508);
    if (x < 0 || y < 0 || w < 30 || h < 30 || w > 40000 || h > 40000) { err(out, "bad-request", "area"); return -1; }
    char body[2048];
    int bl = snprintf(body, sizeof body,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
        "<scan:ScanSettings xmlns:scan=\"http://schemas.hp.com/imaging/escl/2011/05/03\" xmlns:pwg=\"http://www.pwg.org/schemas/2010/12/sm\">\n"
        "<pwg:Version>2.6</pwg:Version>\n"
        "<pwg:ScanRegions><pwg:ScanRegion><pwg:Height>%ld</pwg:Height><pwg:ContentRegionUnits>escl:ThreeHundredthsOfInches</pwg:ContentRegionUnits>"
        "<pwg:Width>%ld</pwg:Width><pwg:XOffset>%ld</pwg:XOffset><pwg:YOffset>%ld</pwg:YOffset></pwg:ScanRegion></pwg:ScanRegions>\n"
        "<pwg:InputSource>%s</pwg:InputSource>\n<scan:ColorMode>%s</scan:ColorMode>\n"
        "<scan:XResolution>%d</scan:XResolution>\n<scan:YResolution>%d</scan:YResolution>\n"
        "<pwg:DocumentFormat>%s</pwg:DocumentFormat>\n<scan:DocumentFormatExt>%s</scan:DocumentFormatExt>\n"
        "</scan:ScanSettings>\n", h, w, x, y, source, color, res, res, format, format);
    char path[128], host[64];
    snprintf(path, sizeof path, "/%s/ScanJobs", rs);
    snprintf(host, sizeof host, "%s:%d", ip, port);
    int fd = phttp_tcp(ip, port, 4000);
    if (fd < 0) { err(out, "unreachable", ""); return -1; }
    PResp r;
    int x2 = phttp_request(fd, "POST", host, path, NULL, "text/xml", body, (size_t)bl, -1, 0, 1 << 16, 15000, &r);
    close(fd);
    if (x2) { err(out, "unreachable", ""); return -1; }
    if (r.status == 503) { presp_free(&r); err(out, "busy", ""); return -1; }
    if (r.status != 201 || !r.location[0]) { char d[16]; snprintf(d, sizeof d, "%d", r.status); presp_free(&r); err(out, "http", d); return -1; }
    /* Location: http://ip:port/eSCL/ScanJobs/uuid or just the path; the path is used, always with
     * the same address (it doesn't follow to another machine) */
    char job[512];
    const char *loc = r.location;
    if (!strncmp(loc, "http://", 7)) { loc = strchr(loc + 7, '/'); if (!loc) loc = "/"; }
    snprintf(job, sizeof job, "%s", loc);
    presp_free(&r);
    for (char *p = job; *p; p++) if ((unsigned char)*p <= 0x20) { *p = 0; break; }
    if (job[0] != '/' || strstr(job, "..")) { err(out, "bad-response", "location"); return -1; }
    char next[600];
    snprintf(next, sizeof next, "%s%sNextDocument", job, job[strlen(job) - 1] == '/' ? "" : "/");
    PResp d;
    int got = 0;
    for (int tries = 0; tries < 40 && !got; tries++) {
        if (get(ip, port, next, &d, 120000)) { err(out, "unreachable", ""); return -1; }
        if (d.status == 200 && d.body && d.len) { got = 1; break; }
        int st = d.status;
        presp_free(&d);
        if (st != 503) { char e[16]; snprintf(e, sizeof e, "%d", st); err(out, "http", e); return -1; }
        struct timespec ts = { 0, 500 * 1000000L };
        nanosleep(&ts, NULL);
    }
    if (!got) { err(out, "timeout", ""); return -1; }
    const char *ext = ext_of(d.content_type) ? ext_of(d.content_type) : ext_of(format);
    /* new file in the scanner's cache folder */
    char file[1024];
    int ofd = -1;
    for (int i = 0; i < 100 && ofd < 0; i++) {
        snprintf(file, sizeof file, "%s/scan-%ld-%d.%s", dir, (long)time(NULL), i, ext);
        ofd = open(file, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
        if (ofd < 0 && errno != EEXIST) break;
    }
    if (ofd < 0) { presp_free(&d); err(out, "write", strerror(errno)); return -1; }
    size_t off = 0;
    while (off < d.len) {
        ssize_t k = write(ofd, d.body + off, d.len - off);
        if (k <= 0) { if (k < 0 && errno == EINTR) continue; break; }
        off += (size_t)k;
    }
    close(ofd);
    int complete = off == d.len;
    jb_free(out);
    jb_raw(out, "{\"file\":");
    jb_str(out, file);
    jb_printf(out, ",\"format\":\"%s\",\"bytes\":%zu}", ext, d.len);
    presp_free(&d);
    return complete ? 0 : -1;
}

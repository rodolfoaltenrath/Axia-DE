#!/usr/bin/env python3
"""escl_server.py — test eSCL (AirScan) scanner for the LordOS Scanner (docs/PRINTING.md).

There is no real scanner in the VM: this server answers like one. It returns capabilities,
accepts jobs (POST /eSCL/ScanJobs) and delivers a "scanned page" built here: a PNG with a
header, ruled lines, a circle and a frame, at the requested area and resolution. Standard
library only (no PIL), so the only format it delivers is PNG; LordOS converts it to PDF or JPEG
in its confined helper.

Usage:  python3 -I escl_server.py [port]          (default 8090)
and announce it over mDNS, for example with avahi:
      avahi-publish -s "Escáner de prueba" _uscan._tcp 8090 "rs=eSCL" "ty=Escáner de prueba LordOS"
"""
import http.server, socketserver, struct, sys, uuid, zlib, re

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8090
MAXW, MAXH = 2550, 3508          # A4 in 1/300 inch
JOBS = {}

CAPS = f"""<?xml version="1.0" encoding="UTF-8"?>
<scan:ScannerCapabilities xmlns:scan="http://schemas.hp.com/imaging/escl/2011/05/03" xmlns:pwg="http://www.pwg.org/schemas/2010/12/sm">
<pwg:Version>2.63</pwg:Version>
<pwg:MakeAndModel>Escáner de prueba LordOS</pwg:MakeAndModel>
<pwg:SerialNumber>LORD-0001</pwg:SerialNumber>
<scan:Platen><scan:PlatenInputCaps>
<scan:MinWidth>16</scan:MinWidth><scan:MaxWidth>{MAXW}</scan:MaxWidth><scan:MinHeight>16</scan:MinHeight><scan:MaxHeight>{MAXH}</scan:MaxHeight>
<scan:SettingProfiles><scan:SettingProfile>
<scan:ColorModes><scan:ColorMode>RGB24</scan:ColorMode><scan:ColorMode>Grayscale8</scan:ColorMode></scan:ColorModes>
<scan:DocumentFormats><pwg:DocumentFormat>image/png</pwg:DocumentFormat><scan:DocumentFormatExt>image/png</scan:DocumentFormatExt></scan:DocumentFormats>
<scan:SupportedResolutions><scan:DiscreteResolutions>
<scan:DiscreteResolution><scan:XResolution>75</scan:XResolution><scan:YResolution>75</scan:YResolution></scan:DiscreteResolution>
<scan:DiscreteResolution><scan:XResolution>150</scan:XResolution><scan:YResolution>150</scan:YResolution></scan:DiscreteResolution>
<scan:DiscreteResolution><scan:XResolution>300</scan:XResolution><scan:YResolution>300</scan:YResolution></scan:DiscreteResolution>
</scan:DiscreteResolutions></scan:SupportedResolutions>
</scan:SettingProfile></scan:SettingProfiles>
</scan:PlatenInputCaps></scan:Platen>
</scan:ScannerCapabilities>
"""

def tag(xml, name, default):
    m = re.search(r"<(?:\w+:)?%s>([^<]*)<" % name, xml)
    return m.group(1) if m else default

def png(w, h, gray, x0, y0, dpi):
    """The page: whole-page coordinates in 1/300", cropped to the requested area. Each row is
    built from spans (fast even in pure Python)."""
    s = dpi / 300.0
    px = lambda c: bytes([int(0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2])]) if gray else bytes(c)
    W, BLUE, GREY, RED, BLACK = px((255, 255, 255)), px((40, 110, 220)), px((90, 90, 90)), px((220, 40, 60)), px((0, 0, 0))
    def col(xpage): return max(0, min(w, int(round((xpage - x0) * s))))   # image column
    rows = []
    for j in range(h):
        py = y0 + j / s
        runs = [(0, w, W)]
        def paint(a, b, c):
            nonlocal runs
            ca, cb = col(a), col(b)
            if cb <= ca: return
            out = []
            for (u, v, cc) in runs:
                if v <= ca or u >= cb: out.append((u, v, cc)); continue
                if u < ca: out.append((u, ca, cc))
                out.append((max(u, ca), min(v, cb), c))
                if v > cb: out.append((cb, v, cc))
            runs = out
        if 150 < py < 420: paint(150, MAXW - 150, BLUE)
        elif 600 < py < 3000 and int((py - 600) // 70) % 2 == 0 and (py - 600) % 70 < 28: paint(250, MAXW - 250 - (300 if int((py - 600) // 70) % 5 == 4 else 0), GREY)
        dy = py - 2950
        if abs(dy) < 260:
            dx = (260 ** 2 - dy ** 2) ** 0.5
            paint(1900 - dx, 1900 + dx, RED)
        if py < 30 or py > MAXH - 30: paint(0, MAXW, BLACK)
        else: paint(0, 30, BLACK); paint(MAXW - 30, MAXW, BLACK)
        rows.append(b"\0" + b"".join(c * (v - u) for (u, v, c) in runs))
    raw = zlib.compress(b"".join(rows), 6)
    def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 0 if gray else 2, 0, 0, 0)) + chunk(b"IDAT", raw) + chunk(b"IEND", b"")

class H(http.server.BaseHTTPRequestHandler):
    def send(self, code, body=b"", ctype="text/xml", extra=None):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        for k, v in (extra or {}).items(): self.send_header(k, v)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/eSCL/ScannerCapabilities": return self.send(200, CAPS.encode())
        if self.path == "/eSCL/ScannerStatus":
            return self.send(200, b'<?xml version="1.0"?><scan:ScannerStatus xmlns:scan="http://schemas.hp.com/imaging/escl/2011/05/03" xmlns:pwg="http://www.pwg.org/schemas/2010/12/sm"><pwg:State>Idle</pwg:State></scan:ScannerStatus>')
        m = re.match(r"^/eSCL/ScanJobs/([0-9a-f-]+)/NextDocument$", self.path)
        if m and m.group(1) in JOBS:
            job = JOBS.pop(m.group(1))
            return self.send(200, job, "image/png")
        self.send(404)

    def do_POST(self):
        if self.path != "/eSCL/ScanJobs": return self.send(404)
        body = self.rfile.read(int(self.headers.get("Content-Length", "0"))).decode("utf-8", "replace")
        dpi = int(tag(body, "XResolution", "150"))
        w, h = int(tag(body, "Width", str(MAXW))), int(tag(body, "Height", str(MAXH)))
        x0, y0 = int(tag(body, "XOffset", "0")), int(tag(body, "YOffset", "0"))
        gray = tag(body, "ColorMode", "RGB24") != "RGB24"
        pw, ph = max(1, w * dpi // 300), max(1, h * dpi // 300)
        print(f"job: {pw}x{ph} at {dpi} dpi, {'grayscale' if gray else 'color'}, from ({x0},{y0})", flush=True)
        jid = str(uuid.uuid4())
        JOBS[jid] = png(pw, ph, gray, x0, y0, dpi)
        self.send(201, b"", extra={"Location": f"http://{self.headers.get('Host', 'localhost')}/eSCL/ScanJobs/{jid}"})

    def log_message(self, fmt, *args): pass

class S(socketserver.ThreadingMixIn, http.server.HTTPServer): daemon_threads = True

if __name__ == "__main__":
    print(f"test eSCL scanner on port {PORT}", flush=True)
    S(("", PORT), H).serve_forever()

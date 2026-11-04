# Live Dock previews

In the default theme, hover over an open app for 220 ms to show its windows.
Move into the panel to keep it open, click a window to focus it, or leave for
300 ms to dismiss it. Escape and an outside click also close it. Paging keeps
all windows accessible without making the panel wider than the display. Dock
The panel holds its opening position while Dock magnification settles. Dock
menus and dragging suppress the preview; auto-hide stays open while using it.

The panel reuses `dockGlass`, including its light/dark tint, outline, blur and
effects-disabled material. There is no separate approximation of that material.

`rust/axia-shell/src/dock_preview.rs` owns hover timing, grace, layout, pagination,
source signature tracking and the aggregate capture budget. The SDK binds window
metadata, input and native image handles to that policy. The C bridge only reads
compositor objects and fills bounded node-owned images; it never captures the
screen, writes PNG files, changes focus, configures clients or asks apps to render.

Only the visible page is captured. A shared interval starts at 67 ms (at most
15 captures/second across all cards), with round-robin selection. Changed node
and committed surface generations invalidate thumbnails; unchanged content does
not get repainted. Capture duration increases the interval to target roughly
3% of one UI thread, capped at one second between captures. This is adaptive
scheduling, not a guarantee about total desktop/GPU load or a hard latency bound
for an individual GPU readback. Image memory is limited to four small cards.

Shared-memory Wayland content is downsampled directly. For GPU dmabuf content,
the existing texture is rendered into a small framebuffer before readback; no
full-resolution GPU screenshot is needed. Surface crops and subsurfaces are
preserved. Native UI trees use the existing rasterizer at thumbnail scale.

Previews do not synthesize frame callbacks: a minimized or suspended app can
stop producing content. Linux apps use their retained compositor surfaces even when minimized or their UI frames are unmounted; the last available frame remains visible when a client stops rendering. Native apps without a mounted frame keep the icon/title fallback.
Protected windows show an icon, and protected descendant nodes are blacked out.
No polling or capture timer remains once the panel and hover delay are closed.

Validation includes Rust policy/budget tests, native raster/protection/state tests,
and an isolated interactive smoke test. Hardware-specific GPU import and readback
performance still needs validation on the target laptop after updating the session.

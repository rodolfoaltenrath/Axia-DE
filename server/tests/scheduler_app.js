// Loaded only in the staged calendar package by tools/desktop/check.py.
// This is a real isolated app: React schedules updates while Wayland controls presentation.
(function () {
  const React = __lordos.require('react');
  const { View, Text } = __lordos.require('@lordos/runtime');
  const sdk = __lordos.require('@lordos/sdk');
  const h = React.createElement;
  globalThis.__lordosApp = {
    default: function SchedulerProbe() {
      const [frame, setFrame] = React.useState(0);
      React.useEffect(() => {
        let stopped = false, request = 0, frames = 0, immediate = 0;
        const step = () => {
          if (stopped) return;
          frames++;
          setFrame((n) => n + 1);
          setImmediate(() => { immediate++; });
          request = requestAnimationFrame(step);
        };
        request = requestAnimationFrame(step);
        const timer = setTimeout(() => {
          stopped = true; cancelAnimationFrame(request);
          if (frames < 1 || frames > 110 || immediate < frames - 1) {
            console.error('scheduler app: unpaced or stalled frames: ' + JSON.stringify({ frames, immediate }));
          } else {
            console.log('scheduler isolated integration: passed ' + JSON.stringify({ frames, immediate }));
          }
          for (const window of sdk.windows.get()) sdk.wm.close(window.id, true);
        }, 1200);
        return () => { stopped = true; cancelAnimationFrame(request); clearTimeout(timer); };
      }, []);
      return h(View, { style: { width: 360, height: 180, backgroundColor: frame % 2 ? '#375867' : '#613f56' } },
        h(Text, { style: { color: '#fff', padding: 20 } }, 'Native scheduler frame ' + frame));
    },
  };
})();

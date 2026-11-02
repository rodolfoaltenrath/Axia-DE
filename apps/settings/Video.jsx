// Settings → Displays → Video in the browser: "Optimize video for this computer" (Automatic, Always,
// Never) and "Lower quality automatically when video stutters" (docs/VIDEO.md). The SDK (sdk/video.js)
// finds out what the hardware decodes and writes the browsers' settings with the closed admin
// action video-optimize.
import { useLocale, useStore, Toggle, videoPrefs, setVideoPrefs, videoHardware, videoError, videoOptimizing, useSettings } from '@lordos/sdk';
import { Section, SettingRow, PopUp, Note } from './ui.jsx';

function statusText(tr, prefs, hw) {
  if (prefs.mode === 'always') return tr('prefs.video.statusAlways');
  if (prefs.mode === 'never') return tr('prefs.video.statusNever');
  if (!hw) return tr('prefs.video.checking');
  if (!videoOptimizing(prefs, hw)) return tr('prefs.video.statusHardware');
  const which = !hw.vp9 && !hw.av1 ? tr('prefs.video.codecsBoth') : !hw.vp9 ? 'VP9' : 'AV1';
  const why = tr(`prefs.video.reason.${hw.reason === 'virtualGpu' ? 'virtualGpu' : hw.reason === 'noDriver' ? 'noDriver' : 'noProfiles'}`);
  return tr('prefs.video.statusAuto', { codecs: which, reason: why });
}

export function BrowserVideo() {
  const { t: tr } = useLocale();
  const [s] = useSettings();
  const prefs = videoPrefs(s);
  const hw = useStore(videoHardware);
  const err = useStore(videoError);
  const modes = [['auto', tr('prefs.video.mode.auto')], ['always', tr('prefs.video.mode.always')], ['never', tr('prefs.video.mode.never')]];
  return (
    <Section title={tr('prefs.video.title')} footer={tr('prefs.video.footer')}>
      <SettingRow label={tr('prefs.video.optimize')} hint={statusText(tr, prefs, hw)}>
        <PopUp width={170} value={prefs.mode} onChange={(mode) => setVideoPrefs({ mode })} options={modes} />
      </SettingRow>
      <SettingRow label={tr('prefs.video.autoLower')} hint={tr('prefs.video.autoLowerHint')}>
        <Toggle small on={prefs.autoLower} onChange={(autoLower) => setVideoPrefs({ autoLower })} />
      </SettingRow>
      {err ? <Note tone="warn">{err}</Note> : null}
    </Section>
  );
}

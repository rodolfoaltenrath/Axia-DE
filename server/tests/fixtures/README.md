`motion-vp8.webm` is the mandatory playback fixture: a generated two-second
solid-color VP8 clip, 1920×1080 at 30 frames/second, without audio. Fedora's
standard codec packages can decode it, including Copr buildroots using
`noopenh264`. It exercises the real wallpaper resolution/frame-rate budgets,
pause/resume, looping, and uncapped ordinary playback without skipping checks.
Reproduce with:

```
ffmpeg -f lavfi -i color=c=0x54789a:size=1920x1080:rate=30 -t 2 \
  -c:v libvpx -b:v 100k -threads 2 -an motion-vp8.webm
```

`motion-h264.mp4` is a generated two-second solid-color H.264 clip, 1920×1080 at
30 frames/second, without audio. It has no third-party image content. Its original
size deliberately exceeds the economical wallpaper output budget. Reproduce with:

```
ffmpeg -f lavfi -i color=c=0x54789a:size=1920x1080:rate=30 -t 2 \
  -c:v libopenh264 -b:v 100k -threads 2 -an motion-h264.mp4
```

The H.264 clip is also used by the offline wallpaper catalog storage tests.
To additionally test its actual playback on a host with an H.264 decoder, run
`python3 server/tests/motion_playback_test.py --h264`. The mandatory checks use
VP8 because Copr does not provide the Cisco H.264 runtime.

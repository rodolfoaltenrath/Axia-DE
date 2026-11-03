`motion-h264.mp4` is a generated two-second solid-color H.264 clip, 1920×1080 at
30 frames/second, without audio. It has no third-party image content. Its original
size deliberately exceeds the economical wallpaper output budget. Reproduce with:

```
ffmpeg -f lavfi -i color=c=0x54789a:size=1920x1080:rate=30 -t 2 \
  -c:v libopenh264 -b:v 100k -threads 2 -an motion-h264.mp4
```

# ffmpeg sidecar

Place platform-named ffmpeg binaries here for bundling with Tauri `externalBin`:

- Linux: `ffmpeg-x86_64-unknown-linux-gnu`
- macOS Intel: `ffmpeg-x86_64-apple-darwin`
- macOS ARM: `ffmpeg-aarch64-apple-darwin`
- Windows: `ffmpeg-x86_64-pc-windows-msvc.exe`

At runtime the app also falls back to `ffmpeg` on `PATH` if no sidecar is found.

Download static builds from https://ffmpeg.org/download.html or trusted static builds (e.g. johnvansickle / BtbN).

After placing binaries, add to `tauri.conf.json`:

```json
"bundle": {
  "externalBin": ["binaries/ffmpeg"]
}
```

(Tauri renames `binaries/ffmpeg` to the target-triple form automatically at build time.)

# ffmpeg sidecar（安装包内置）

构建时由 `node scripts/prepare-ffmpeg.mjs` 自动下载对应平台的**静态** ffmpeg，经 Tauri `bundle.externalBin` 打进安装包。

用户安装后即可合并 MP4，**无需**自行寻找或配置 ffmpeg。

开发调试可用系统 PATH 中的 `ffmpeg`；打包前脚本会按目标平台拉取 sidecar。

文件命名（gitignore，不入库）：

- Linux x64: `ffmpeg-x86_64-unknown-linux-gnu`
- Linux ARM64: `ffmpeg-aarch64-unknown-linux-gnu`
- macOS Intel: `ffmpeg-x86_64-apple-darwin`
- macOS Apple Silicon: `ffmpeg-aarch64-apple-darwin`
- Windows x64: `ffmpeg-x86_64-pc-windows-msvc.exe`

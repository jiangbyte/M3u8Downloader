# M3U8 Downloader

![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.77%2B-orange?logo=rust&logoColor=white)
![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)
![TypeScript](https://img.shields.io/badge/TypeScript-Supported-3178C6?logo=typescript&logoColor=white)
![Ant Design](https://img.shields.io/badge/UI-Ant%20Design-0170FE?logo=antdesign&logoColor=white)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)
![License](https://img.shields.io/badge/License-MIT-green)
![Version](https://img.shields.io/badge/version-0.1.0-orange)

**M3U8 Downloader** 是一款跨平台桌面端 m3u8 多线程下载器：支持可选 AES-128 解密、自定义请求头 / Cookie / 代理，任务级与分片级控制，下载完成后自动 remux 为 MP4，并支持边下边播。

> 当前版本：`0.1.0` · 协议：[MIT License](LICENSE)

## 目录

- [功能特性](#功能特性)
- [技术栈](#技术栈)
- [工程结构](#工程结构)
- [快速开始](#快速开始)
- [常用命令](#常用命令)
- [ffmpeg](#ffmpeg)
- [License](#license)

## 功能特性

| 模块 | 说明 |
| --- | --- |
| 播放列表 | 解析 master / media playlist；master 时可选择清晰度变体 |
| 加密分片 | 未加密与 AES-128（`#EXT-X-KEY`）分片下载与解密 |
| 请求选项 | 可选自定义 Headers、Cookie、Referer、HTTP(S) 代理、并发数、输出目录 / 文件名 |
| 任务管理 | 多任务队列；暂停 / 继续 / 取消 / 删除；按已下分片断点续传 |
| 分片控制 | 任务详情中单独开始 / 停止 / 重试分片；任务级操作会中断在途下载 |
| 边下边播 | 基于本地 HLS 服务播放已连续下载的分片；完成后可直接打开 MP4 |
| 合并输出 | ffmpeg `-c copy` 将 TS 分片 remux 为 MP4；可选清理临时分片 |

## 技术栈

| 层级 | 技术 |
| --- | --- |
| 桌面壳 | Tauri 2 |
| 下载引擎 | Rust · Tokio · reqwest |
| 前端 | React 19 · TypeScript · Vite · Ant Design |
| 合并 | 安装包内置 ffmpeg sidecar（开发可用系统 PATH） |

## 工程结构

```text
M3u8Downloader/
├── src/                      # React UI
│   ├── components/           # 新建任务、任务列表 / 分片详情
│   ├── api.ts                # Tauri invoke 封装
│   └── types.ts
├── src-tauri/
│   ├── src/
│   │   ├── m3u8/             # playlist 解析
│   │   ├── download/         # 并发下载池、分片控制
│   │   ├── crypto/           # AES-128-CBC
│   │   ├── merge/            # ffmpeg remux
│   │   ├── play/             # 边下边播本地 HLS
│   │   ├── task/             # 任务状态与持久化
│   │   └── commands.rs       # Tauri commands
│   └── binaries/             # 构建时自动下载的 ffmpeg sidecar
└── scripts/prepare-ffmpeg.mjs
```

## 快速开始

### 环境要求

- Node.js（建议配合 pnpm）
- Rust / Cargo（建议放在 `~/workspace/sdks`，见 `Rust安装记录.md` / `environment.d/rust.conf`）
- Linux 还需 Tauri 系统依赖，例如：

```bash
sudo apt install libwebkit2gtk-4.1-dev librsvg2-dev patchelf \
  libssl-dev libayatana-appindicator3-dev
```

> 发布安装包已内置 ffmpeg。本地 `tauri:dev` 若需合并，可另装系统 `ffmpeg`，或先执行 `pnpm prepare:ffmpeg`。

### 启动

```bash
pnpm install
pnpm tauri:dev
```

### 打包

```bash
pnpm tauri:build
```

产物目标见 `src-tauri/tauri.conf.json`（deb / AppImage / msi / nsis / dmg）。

各平台安装包见 [Releases](https://github.com/jiangbyte/M3u8Downloader/releases)。

## 常用命令

| 命令 | 说明 |
| --- | --- |
| `pnpm tauri:dev` | 开发模式启动桌面应用 |
| `pnpm tauri:build` | 构建发布包（自动下载并内置 ffmpeg） |
| `pnpm prepare:ffmpeg` | 仅下载当前平台 ffmpeg sidecar |
| `pnpm build` | 仅构建前端静态资源 |
| `pnpm lint` | 前端 lint（oxlint） |

## ffmpeg

**Windows / macOS / Linux 安装包均已内置静态 ffmpeg**，安装后即可合并 MP4，无需用户自行下载或放置。

本地开发：

- `pnpm tauri:dev`：优先使用系统 PATH 中的 `ffmpeg`
- `pnpm tauri:build`：构建前自动执行 `scripts/prepare-ffmpeg.mjs` 并打包进安装程序

边下边播优先调用本机 `mpv` / `ffplay` / `vlc`，否则回退系统默认打开方式。

## License

本项目基于 [MIT License](LICENSE) 开源。完整条款见 [LICENSE](LICENSE)。

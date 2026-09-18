#!/usr/bin/env node
/**
 * Download a static ffmpeg binary for the current (or TARGET_TRIPLE) platform
 * into src-tauri/binaries/ for Tauri bundle.externalBin.
 *
 * Env:
 *   TARGET_TRIPLE / TAURI_ENV_TARGET_TRIPLE — cross-compile target
 *   FORCE_FFMPEG=1 — re-download even if present
 */
import { createWriteStream, existsSync, mkdirSync, chmodSync } from 'node:fs'
import { pipeline } from 'node:stream/promises'
import { createGunzip } from 'node:zlib'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { arch, platform } from 'node:os'

const __dirname = dirname(fileURLToPath(import.meta.url))
const ROOT = join(__dirname, '..')
const OUT_DIR = join(ROOT, 'src-tauri', 'binaries')
const RELEASE = 'b6.1.1'
const BASE = `https://github.com/eugeneware/ffmpeg-static/releases/download/${RELEASE}`

/** @type {Record<string, { asset: string, dest: string }>} */
const BY_TRIPLE = {
  'x86_64-unknown-linux-gnu': {
    asset: 'ffmpeg-linux-x64.gz',
    dest: 'ffmpeg-x86_64-unknown-linux-gnu',
  },
  'aarch64-unknown-linux-gnu': {
    asset: 'ffmpeg-linux-arm64.gz',
    dest: 'ffmpeg-aarch64-unknown-linux-gnu',
  },
  'x86_64-apple-darwin': {
    asset: 'ffmpeg-darwin-x64.gz',
    dest: 'ffmpeg-x86_64-apple-darwin',
  },
  'aarch64-apple-darwin': {
    asset: 'ffmpeg-darwin-arm64.gz',
    dest: 'ffmpeg-aarch64-apple-darwin',
  },
  'x86_64-pc-windows-msvc': {
    asset: 'ffmpeg-win32-x64.gz',
    dest: 'ffmpeg-x86_64-pc-windows-msvc.exe',
  },
}

function inferTriple() {
  const fromEnv =
    process.env.TARGET_TRIPLE ||
    process.env.TAURI_ENV_TARGET_TRIPLE ||
    ''
  if (fromEnv && BY_TRIPLE[fromEnv]) return fromEnv

  const p = platform()
  const a = arch()
  if (p === 'linux' && a === 'x64') return 'x86_64-unknown-linux-gnu'
  if (p === 'linux' && (a === 'arm64' || a === 'aarch64'))
    return 'aarch64-unknown-linux-gnu'
  if (p === 'darwin' && a === 'arm64') return 'aarch64-apple-darwin'
  if (p === 'darwin' && a === 'x64') return 'x86_64-apple-darwin'
  if (p === 'win32' && a === 'x64') return 'x86_64-pc-windows-msvc'
  throw new Error(
    `Unsupported host platform ${p}/${a}; set TARGET_TRIPLE to one of: ${Object.keys(BY_TRIPLE).join(', ')}`,
  )
}

async function download(url, destPath) {
  const res = await fetch(url, { redirect: 'follow' })
  if (!res.ok || !res.body) {
    throw new Error(`Download failed ${res.status} ${url}`)
  }
  await pipeline(res.body, createGunzip(), createWriteStream(destPath))
}

async function main() {
  const triple = inferTriple()
  const spec = BY_TRIPLE[triple]
  mkdirSync(OUT_DIR, { recursive: true })
  const destPath = join(OUT_DIR, spec.dest)

  if (existsSync(destPath) && process.env.FORCE_FFMPEG !== '1') {
    console.log(`ffmpeg already present: ${destPath}`)
    return
  }

  const url = `${BASE}/${spec.asset}`
  console.log(`Downloading ffmpeg for ${triple}`)
  console.log(`  ${url}`)
  await download(url, destPath)
  if (platform() !== 'win32') {
    chmodSync(destPath, 0o755)
  }
  console.log(`ffmpeg sidecar ready: ${destPath}`)
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})

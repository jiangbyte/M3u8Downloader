export type TaskStatus =
  | 'queued'
  | 'downloading'
  | 'paused'
  | 'merging'
  | 'completed'
  | 'failed'
  | 'cancelled'

export interface RequestOptions {
  headers?: Record<string, string>
  cookie?: string | null
  proxy?: string | null
  referer?: string | null
}

export interface VariantInfo {
  url: string
  bandwidth?: number | null
  resolution?: string | null
  name?: string | null
  codecs?: string | null
}

export interface AnalyzeResult {
  kind: 'master' | 'media'
  url: string
  variants: VariantInfo[]
  media?: unknown
}

export interface TaskSnapshot {
  id: string
  title: string
  url: string
  status: TaskStatus
  progress: number
  downloadedSegments: number
  totalSegments: number
  speedBps: number
  etaSecs?: number | null
  outputPath?: string | null
  workDir: string
  error?: string | null
  createdAt: string
}

export type SegmentState =
  | 'pending'
  | 'downloading'
  | 'done'
  | 'failed'
  | 'cancelled'

export interface SegmentStatus {
  index: number
  name: string
  state: SegmentState
  size: number
  error?: string | null
}

export interface TaskDetail {
  task: TaskSnapshot
  segments: SegmentStatus[]
  doneCount: number
  pendingCount: number
}

export interface StartTaskInput {
  url: string
  selectedVariantUrl?: string | null
  outputDir?: string | null
  filename?: string | null
  concurrency?: number | null
  cleanupSegments?: boolean | null
  options: RequestOptions
}

export interface NewTaskFormValues {
  url: string
  filename?: string
  outputDir?: string
  concurrency?: number
  cookie?: string
  proxy?: string
  referer?: string
  headersText?: string
  cleanupSegments?: boolean
  jumpToTasks?: boolean
}

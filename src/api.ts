import { invoke } from '@tauri-apps/api/core'
import type {
  AnalyzeResult,
  RequestOptions,
  StartTaskInput,
  TaskDetail,
  TaskSnapshot,
} from './types'

export function analyzeUrl(url: string, options: RequestOptions) {
  return invoke<AnalyzeResult>('analyze_url', { url, options })
}

export function startTask(input: StartTaskInput) {
  return invoke<TaskSnapshot>('start_task', { input })
}

export function listTasks() {
  return invoke<TaskSnapshot[]>('list_tasks')
}

export function getTaskDetail(id: string) {
  return invoke<TaskDetail>('get_task_detail', { id })
}

export function pauseTask(id: string) {
  return invoke<TaskSnapshot>('pause_task', { id })
}

export function resumeTask(id: string) {
  return invoke<TaskSnapshot>('resume_task', { id })
}

export function cancelTask(id: string) {
  return invoke<TaskSnapshot>('cancel_task', { id })
}

export function removeTask(id: string, deleteFiles = false) {
  return invoke<void>('remove_task', { id, deleteFiles })
}

export function startSegment(id: string, index: number) {
  return invoke<void>('start_segment', { id, index })
}

export function cancelSegment(id: string, index: number) {
  return invoke<void>('cancel_segment', { id, index })
}

export function retrySegment(id: string, index: number) {
  return invoke<void>('retry_segment', { id, index })
}

export function playTask(id: string) {
  return invoke<string>('play_task', { id })
}

export function openPath(path: string) {
  return invoke<void>('open_path', { path })
}

export function defaultOutputDir() {
  return invoke<string>('default_output_dir')
}

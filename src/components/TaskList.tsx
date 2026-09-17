import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Button,
  Empty,
  Modal,
  Popconfirm,
  Progress,
  Space,
  Table,
  Tag,
  Tooltip,
  Typography,
  message,
} from 'antd'
import type { ColumnsType } from 'antd/es/table'
import {
  CheckCircleOutlined,
  ClockCircleOutlined,
  CloseCircleOutlined,
  DeleteOutlined,
  FolderOpenOutlined,
  LoadingOutlined,
  PauseCircleOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
  StopOutlined,
  VideoCameraOutlined,
} from '@ant-design/icons'
import { listen } from '@tauri-apps/api/event'
import type {
  SegmentState,
  SegmentStatus,
  TaskDetail,
  TaskSnapshot,
  TaskStatus,
} from '../types'
import {
  cancelSegment,
  cancelTask,
  getTaskDetail,
  openPath,
  pauseTask,
  playTask,
  removeTask,
  resumeTask,
  retrySegment,
  startSegment,
} from '../api'

function statusTag(status: TaskStatus) {
  const map: Record<TaskStatus, { color: string; text: string }> = {
    queued: { color: 'default', text: '排队' },
    downloading: { color: 'processing', text: '下载' },
    paused: { color: 'warning', text: '暂停' },
    merging: { color: 'processing', text: '合并' },
    completed: { color: 'success', text: '完成' },
    failed: { color: 'error', text: '失败' },
    cancelled: { color: 'default', text: '已取消' },
  }
  const item = map[status]
  return <Tag color={item.color}>{item.text}</Tag>
}

function segmentStateLabel(state: SegmentState) {
  switch (state) {
    case 'done':
      return '已完成'
    case 'downloading':
      return '下载中'
    case 'failed':
      return '失败'
    case 'cancelled':
      return '已停止'
    default:
      return '等待中'
  }
}

function formatSpeed(bps: number) {
  if (!bps || bps <= 0) return '-'
  if (bps < 1024) return `${bps.toFixed(0)} B/s`
  if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`
  return `${(bps / 1024 / 1024).toFixed(2)} MB/s`
}

function formatEta(secs?: number | null) {
  if (secs == null) return '-'
  if (secs < 60) return `${secs}s`
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  return `${h}h ${m}m`
}

function formatSize(bytes: number) {
  if (!bytes) return '-'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`
}

function progressStatus(status: TaskStatus) {
  if (status === 'failed') return 'exception' as const
  if (status === 'completed') return 'success' as const
  if (status === 'paused' || status === 'cancelled') return 'normal' as const
  return 'active' as const
}

interface Props {
  tasks: TaskSnapshot[]
  onChange: () => void
}

export default function TaskList({ tasks, onChange }: Props) {
  const [activeId, setActiveId] = useState<string | null>(null)
  const [detail, setDetail] = useState<TaskDetail | null>(null)
  const [loadingDetail, setLoadingDetail] = useState(false)
  const [busySegment, setBusySegment] = useState<number | null>(null)

  const liveTask = useMemo(
    () => (activeId ? tasks.find((t) => t.id === activeId) : undefined),
    [activeId, tasks],
  )

  const refreshDetail = useCallback(async (id: string) => {
    setLoadingDetail(true)
    try {
      const d = await getTaskDetail(id)
      setDetail(d)
    } catch (e) {
      message.error(String(e))
    } finally {
      setLoadingDetail(false)
    }
  }, [])

  useEffect(() => {
    if (!activeId) {
      setDetail(null)
      return
    }
    void refreshDetail(activeId)
  }, [activeId, refreshDetail])

  useEffect(() => {
    if (!activeId || !liveTask) return
    if (!['downloading', 'queued', 'merging', 'paused'].includes(liveTask.status))
      return
    const timer = window.setInterval(() => {
      void refreshDetail(activeId)
    }, 1000)
    return () => window.clearInterval(timer)
  }, [activeId, liveTask?.status, refreshDetail])

  useEffect(() => {
    let unlisten: (() => void) | undefined
    void listen<string>('segment-update', (event) => {
      if (activeId && event.payload === activeId) {
        void refreshDetail(activeId)
      }
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
    }
  }, [activeId, refreshDetail])

  const shownTask = liveTask ?? detail?.task

  async function runAction(action: () => Promise<unknown>) {
    try {
      await action()
      onChange()
      if (activeId) await refreshDetail(activeId)
    } catch (e) {
      message.error(String(e))
    }
  }

  async function runSegmentAction(
    index: number,
    action: () => Promise<unknown>,
  ) {
    setBusySegment(index)
    try {
      await action()
      if (activeId) await refreshDetail(activeId)
      onChange()
    } catch (e) {
      message.error(String(e))
    } finally {
      setBusySegment(null)
    }
  }

  const columns: ColumnsType<TaskSnapshot> = useMemo(
    () => [
      {
        title: '任务',
        dataIndex: 'title',
        key: 'title',
        ellipsis: true,
        render: (_: unknown, task) => (
          <div className="task-cell-title">
            <Typography.Text strong ellipsis>
              {task.title}
            </Typography.Text>
            <Typography.Text type="secondary" ellipsis className="task-cell-url">
              {task.url}
            </Typography.Text>
          </div>
        ),
      },
      {
        title: '状态',
        dataIndex: 'status',
        key: 'status',
        width: 88,
        render: (status: TaskStatus) => statusTag(status),
      },
      {
        title: '进度',
        dataIndex: 'progress',
        key: 'progress',
        width: 160,
        render: (_: unknown, task) => (
          <Progress
            percent={Number(task.progress.toFixed(1))}
            status={progressStatus(task.status)}
            strokeColor="#1677ff"
            trailColor="#f0f0f0"
            size="small"
          />
        ),
      },
      {
        title: '分片',
        key: 'segments',
        width: 100,
        render: (_: unknown, task) => (
          <Typography.Text type="secondary">
            {task.downloadedSegments}/{task.totalSegments}
          </Typography.Text>
        ),
      },
      {
        title: '速度',
        dataIndex: 'speedBps',
        key: 'speed',
        width: 100,
        render: (bps: number) => formatSpeed(bps),
      },
      {
        title: 'ETA',
        dataIndex: 'etaSecs',
        key: 'eta',
        width: 88,
        render: (secs: number | null | undefined) => formatEta(secs),
      },
      {
        title: '操作',
        key: 'actions',
        width: 380,
        fixed: 'right',
        render: (_: unknown, task) => (
          <Space
            size={0}
            wrap={false}
            className="task-table-actions"
            onClick={(e) => e.stopPropagation()}
          >
            {(task.status === 'downloading' || task.status === 'queued') && (
              <Button
                type="link"
                size="small"
                icon={<PauseCircleOutlined />}
                onClick={() => runAction(() => pauseTask(task.id))}
              >
                暂停
              </Button>
            )}
            {(task.status === 'paused' ||
              task.status === 'failed' ||
              task.status === 'cancelled') && (
              <Button
                type="link"
                size="small"
                icon={<PlayCircleOutlined />}
                onClick={() => runAction(() => resumeTask(task.id))}
              >
                继续
              </Button>
            )}
            {(task.status === 'downloading' ||
              task.status === 'queued' ||
              task.status === 'paused' ||
              task.status === 'merging') && (
              <Button
                type="link"
                size="small"
                danger
                icon={<StopOutlined />}
                onClick={() => runAction(() => cancelTask(task.id))}
              >
                取消
              </Button>
            )}
            {task.downloadedSegments > 0 || task.status === 'completed' ? (
              <Button
                type="link"
                size="small"
                icon={<VideoCameraOutlined />}
                onClick={() =>
                  runAction(async () => {
                    await playTask(task.id)
                    message.success('已打开播放器')
                  })
                }
              >
                边下边播
              </Button>
            ) : null}
            <Button
              type="link"
              size="small"
              icon={<FolderOpenOutlined />}
              onClick={() => openPath(task.outputPath || task.workDir)}
            >
              目录
            </Button>
            <Popconfirm
              title="删除此任务？"
              description="将停止下载并删除临时文件"
              onConfirm={() => runAction(() => removeTask(task.id, true))}
              okText="删除"
              cancelText="取消"
            >
              <Button type="link" size="small" danger icon={<DeleteOutlined />}>
                删除
              </Button>
            </Popconfirm>
          </Space>
        ),
      },
    ],
    // runAction closes over latest activeId
    [activeId],
  )

  return (
    <>
      <Table<TaskSnapshot>
        className="task-table"
        rowKey="id"
        size="middle"
        columns={columns}
        dataSource={tasks}
        pagination={false}
        scroll={{ x: 1100, y: 'calc(100vh - 180px)' }}
        locale={{ emptyText: <Empty description="粘贴 m3u8 开始" /> }}
        onRow={(task) => ({
          onClick: () => setActiveId(task.id),
          className: 'task-table-row',
        })}
      />

      <Modal
        title={shownTask ? `分片详情 · ${shownTask.title}` : '分片详情'}
        open={!!activeId}
        onCancel={() => setActiveId(null)}
        width={720}
        footer={null}
        destroyOnHidden
        className="task-detail-modal"
        styles={{ body: { paddingTop: 12, height: 480, overflow: 'hidden' } }}
      >
        {shownTask && activeId ? (
          <TaskDetailBody
            task={shownTask}
            segments={detail?.segments ?? []}
            doneCount={detail?.doneCount ?? shownTask.downloadedSegments}
            pendingCount={
              detail?.pendingCount ??
              Math.max(0, shownTask.totalSegments - shownTask.downloadedSegments)
            }
            loading={loadingDetail}
            busySegment={busySegment}
            onStart={(index) =>
              runSegmentAction(index, () => startSegment(activeId, index))
            }
            onStop={(index) =>
              runSegmentAction(index, () => cancelSegment(activeId, index))
            }
            onRetry={(index) =>
              runSegmentAction(index, () => retrySegment(activeId, index))
            }
          />
        ) : null}
      </Modal>
    </>
  )
}

function SegmentIcon({ state }: { state: SegmentState }) {
  switch (state) {
    case 'done':
      return <CheckCircleOutlined className="segment-icon done" />
    case 'downloading':
      return <LoadingOutlined className="segment-icon downloading" spin />
    case 'failed':
      return <CloseCircleOutlined className="segment-icon failed" />
    case 'cancelled':
      return <StopOutlined className="segment-icon cancelled" />
    default:
      return <ClockCircleOutlined className="segment-icon pending" />
  }
}

function TaskDetailBody({
  task,
  segments,
  doneCount,
  pendingCount,
  loading,
  busySegment,
  onStart,
  onStop,
  onRetry,
}: {
  task: TaskSnapshot
  segments: SegmentStatus[]
  doneCount: number
  pendingCount: number
  loading: boolean
  busySegment: number | null
  onStart: (index: number) => void
  onStop: (index: number) => void
  onRetry: (index: number) => void
}) {
  return (
    <div className="task-detail">
      <div className="task-detail-summary">
        <Space size={8} wrap>
          {statusTag(task.status)}
          <Typography.Text type="secondary">
            已完成 {doneCount} · 待处理 {pendingCount} · 共{' '}
            {task.totalSegments || segments.length}
          </Typography.Text>
        </Space>
        <Progress
          percent={Number(task.progress.toFixed(1))}
          status={progressStatus(task.status)}
          strokeColor="#1677ff"
          trailColor="#f0f0f0"
          size="small"
        />
      </div>

      <div className="task-segment-panel">
        <Typography.Text strong className="task-segment-title">
          全部分片（{segments.length}）
        </Typography.Text>
        <div className="segment-list">
          {loading && !segments.length ? (
            <Typography.Text type="secondary">加载中…</Typography.Text>
          ) : null}
          {!loading && !segments.length ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description="暂无分片信息"
            />
          ) : null}
          {segments.map((seg) => {
            const busy = busySegment === seg.index
            return (
              <div key={seg.index} className={`segment-item is-${seg.state}`}>
                <SegmentIcon state={seg.state} />
                <Typography.Text className="segment-name" ellipsis>
                  {seg.name}
                </Typography.Text>
                <Tooltip title={seg.error || undefined}>
                  <span className="segment-meta">
                    {seg.state === 'done'
                      ? formatSize(seg.size)
                      : segmentStateLabel(seg.state)}
                  </span>
                </Tooltip>
                <Space
                  size={4}
                  className="segment-actions"
                  onClick={(e) => e.stopPropagation()}
                >
                  {(seg.state === 'pending' || seg.state === 'cancelled') && (
                    <Button
                      size="small"
                      type="link"
                      icon={<PlayCircleOutlined />}
                      loading={busy}
                      onClick={() => onStart(seg.index)}
                    >
                      开始
                    </Button>
                  )}
                  {seg.state === 'downloading' && (
                    <Button
                      size="small"
                      type="link"
                      danger
                      icon={<StopOutlined />}
                      loading={busy}
                      onClick={() => onStop(seg.index)}
                    >
                      停止
                    </Button>
                  )}
                  {(seg.state === 'failed' || seg.state === 'done') && (
                    <Button
                      size="small"
                      type="link"
                      icon={<ReloadOutlined />}
                      loading={busy}
                      onClick={() => onRetry(seg.index)}
                    >
                      重试
                    </Button>
                  )}
                </Space>
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}

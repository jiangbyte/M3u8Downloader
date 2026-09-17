import { useCallback, useEffect, useMemo, useState } from 'react'
import { Badge, Layout, Menu, Typography, message } from 'antd'
import {
  DownloadOutlined,
  UnorderedListOutlined,
} from '@ant-design/icons'
import { listen } from '@tauri-apps/api/event'
import NewTaskForm from './components/NewTaskForm'
import TaskList from './components/TaskList'
import { listTasks } from './api'
import type { TaskSnapshot } from './types'
import './App.css'

const { Sider, Content } = Layout

type NavKey = 'new' | 'tasks'

export default function App() {
  const [tasks, setTasks] = useState<TaskSnapshot[]>([])
  const [nav, setNav] = useState<NavKey>('new')

  const refresh = useCallback(async () => {
    try {
      const list = await listTasks()
      setTasks(list)
    } catch (e) {
      message.error(String(e))
    }
  }, [])

  useEffect(() => {
    void refresh()
    let unlisten: (() => void) | undefined
    void listen<TaskSnapshot>('task-progress', (event) => {
      const snap = event.payload
      setTasks((prev) => {
        const idx = prev.findIndex((t) => t.id === snap.id)
        if (idx < 0) return [snap, ...prev]
        const next = [...prev]
        next[idx] = snap
        return next
      })
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
    }
  }, [refresh])

  const activeCount = useMemo(
    () =>
      tasks.filter((t) =>
        ['queued', 'downloading', 'merging', 'paused'].includes(t.status),
      ).length,
    [tasks],
  )

  const menuItems = [
    {
      key: 'new',
      icon: <DownloadOutlined />,
      label: '新建任务',
    },
    {
      key: 'tasks',
      icon: <UnorderedListOutlined />,
      label: (
        <span className="nav-label-with-badge">
          任务列表
          {activeCount > 0 ? (
            <Badge count={activeCount} size="small" color="#1677ff" />
          ) : null}
        </span>
      ),
    },
  ]

  return (
    <Layout className="app-layout">
      <Sider width={220} className="app-sider" theme="light">
        <div className="sider-brand">
          <Typography.Title level={5} className="app-brand">
            M3U8 Downloader
          </Typography.Title>
        </div>
        <Menu
          mode="inline"
          selectedKeys={[nav]}
          items={menuItems}
          onClick={({ key }) => setNav(key as NavKey)}
          className="app-menu"
        />
      </Sider>
      <Layout className="app-main">
        <Content className="app-content">
          {nav === 'new' ? (
            <section className="home-panel">
              <NewTaskForm
                onCreated={({ jumpToTasks } = {}) => {
                  void refresh()
                  if (jumpToTasks !== false) {
                    setNav('tasks')
                  }
                }}
              />
            </section>
          ) : (
            <section className="panel task-panel">
              <Typography.Title level={5} className="panel-title">
                任务列表
              </Typography.Title>
              <TaskList tasks={tasks} onChange={refresh} />
            </section>
          )}
        </Content>
      </Layout>
    </Layout>
  )
}

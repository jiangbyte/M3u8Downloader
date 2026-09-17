import { useMemo, useState } from 'react'
import {
  Button,
  Col,
  Collapse,
  Form,
  Input,
  InputNumber,
  Modal,
  Radio,
  Row,
  Space,
  Switch,
  Typography,
  message,
} from 'antd'
import { DownloadOutlined, FolderOpenOutlined, SettingOutlined } from '@ant-design/icons'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import type { AnalyzeResult, NewTaskFormValues, VariantInfo } from '../types'
import { analyzeUrl, startTask } from '../api'

function parseHeaders(text?: string): Record<string, string> {
  const headers: Record<string, string> = {}
  if (!text?.trim()) return headers
  for (const line of text.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed) continue
    const idx = trimmed.indexOf(':')
    if (idx <= 0) continue
    const key = trimmed.slice(0, idx).trim()
    const value = trimmed.slice(idx + 1).trim()
    if (key) headers[key] = value
  }
  return headers
}

function variantLabel(v: VariantInfo) {
  const parts = [
    v.resolution || undefined,
    v.bandwidth ? `${Math.round(v.bandwidth / 1000)} kbps` : undefined,
    v.name || undefined,
  ].filter(Boolean)
  return parts.join(' · ') || v.url
}

interface Props {
  onCreated: (opts?: { jumpToTasks?: boolean }) => void
}

const JUMP_KEY = 'm3u8.jumpToTasks'
const CONFIG_OPEN_KEY = 'm3u8.configOpen'

function readJumpPref(): boolean {
  try {
    const v = localStorage.getItem(JUMP_KEY)
    if (v === null) return true
    return v === '1'
  } catch {
    return true
  }
}

function writeJumpPref(value: boolean) {
  try {
    localStorage.setItem(JUMP_KEY, value ? '1' : '0')
  } catch {
    /* ignore */
  }
}

function readConfigOpen(): boolean {
  try {
    return localStorage.getItem(CONFIG_OPEN_KEY) === '1'
  } catch {
    return false
  }
}

function writeConfigOpen(open: boolean) {
  try {
    localStorage.setItem(CONFIG_OPEN_KEY, open ? '1' : '0')
  } catch {
    /* ignore */
  }
}

function OutputDirField({
  value,
  onChange,
}: {
  value?: string
  onChange?: (value: string) => void
}) {
  async function browse() {
    try {
      const selected = await openDialog({
        directory: true,
        multiple: false,
        title: '选择输出目录',
        defaultPath: value?.trim() || undefined,
      })
      if (typeof selected === 'string' && selected) {
        onChange?.(selected)
      }
    } catch (e) {
      message.error(String(e))
    }
  }

  return (
    <Space.Compact style={{ width: '100%' }}>
      <Input
        value={value}
        onChange={(e) => onChange?.(e.target.value)}
        placeholder="默认 ~/Downloads/M3U8Downloads"
        allowClear
      />
      <Button icon={<FolderOpenOutlined />} onClick={() => void browse()}>
        浏览
      </Button>
    </Space.Compact>
  )
}

export default function NewTaskForm({ onCreated }: Props) {
  const [form] = Form.useForm<NewTaskFormValues>()
  const [loading, setLoading] = useState(false)
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [configOpen, setConfigOpen] = useState(() => readConfigOpen())
  const [variants, setVariants] = useState<VariantInfo[] | null>(null)
  const [selectedVariant, setSelectedVariant] = useState<string>()
  const [pending, setPending] = useState<NewTaskFormValues | null>(null)

  const initial = useMemo(
    () => ({
      concurrency: 16,
      cleanupSegments: true,
      jumpToTasks: readJumpPref(),
    }),
    [],
  )

  async function submit(values: NewTaskFormValues, variantUrl?: string) {
    setLoading(true)
    try {
      const options = {
        headers: parseHeaders(values.headersText),
        cookie: values.cookie?.trim() || null,
        proxy: values.proxy?.trim() || null,
        referer: values.referer?.trim() || null,
      }
      await startTask({
        url: values.url.trim(),
        selectedVariantUrl: variantUrl || null,
        outputDir: values.outputDir?.trim() || null,
        filename: values.filename?.trim() || null,
        concurrency: values.concurrency ?? 16,
        cleanupSegments: values.cleanupSegments ?? true,
        options,
      })
      const jumpToTasks = values.jumpToTasks ?? true
      writeJumpPref(jumpToTasks)
      message.success('任务已开始')
      form.resetFields()
      form.setFieldsValue({ ...initial, jumpToTasks })
      setAdvancedOpen(false)
      onCreated({ jumpToTasks })
    } catch (e) {
      message.error(String(e))
    } finally {
      setLoading(false)
    }
  }

  async function onFinish(values: NewTaskFormValues) {
    if (!values.url?.trim()) {
      message.warning('请输入 m3u8 地址')
      return
    }
    setLoading(true)
    try {
      const options = {
        headers: parseHeaders(values.headersText),
        cookie: values.cookie?.trim() || null,
        proxy: values.proxy?.trim() || null,
        referer: values.referer?.trim() || null,
      }
      const analyzed: AnalyzeResult = await analyzeUrl(values.url.trim(), options)
      if (analyzed.kind === 'master' && analyzed.variants.length > 0) {
        setPending(values)
        setVariants(analyzed.variants)
        setSelectedVariant(analyzed.variants[0]?.url)
        setLoading(false)
        return
      }
      await submit(values)
    } catch (e) {
      message.error(String(e))
      setLoading(false)
    }
  }

  return (
    <>
      <Form
        form={form}
        layout="vertical"
        initialValues={initial}
        onFinish={onFinish}
        requiredMark={false}
        className="home-form"
      >
        <div className="home-hero">
          <div className="home-brand-block">
            <Typography.Title level={2} className="home-brand">
              M3U8 Downloader
            </Typography.Title>
            <Typography.Paragraph className="home-slogan">
              多线程下载 · 自动合并 MP4 · 可选加密与鉴权
            </Typography.Paragraph>
          </div>

          <div className="home-card">
            <div className="home-search-row">
              <Space.Compact size="large" className="home-compact">
                <Form.Item
                  name="url"
                  rules={[{ required: true, message: '请输入地址' }]}
                  className="home-url-item"
                >
                  <Input
                    allowClear
                    placeholder="粘贴 m3u8 链接后开始下载"
                    onPressEnter={() => form.submit()}
                  />
                </Form.Item>
                <Button
                  type="primary"
                  htmlType="submit"
                  icon={<DownloadOutlined />}
                  loading={loading}
                >
                  开始下载
                </Button>
                <Button
                  icon={<SettingOutlined />}
                  onClick={() => setAdvancedOpen(true)}
                  title="高级选项"
                />
              </Space.Compact>
            </div>

            <Collapse
              ghost
              bordered={false}
              className="home-config-collapse"
              activeKey={configOpen ? ['config'] : []}
              onChange={(keys) => {
                const open = (Array.isArray(keys) ? keys : [keys]).includes('config')
                setConfigOpen(open)
                writeConfigOpen(open)
              }}
              items={[
                {
                  key: 'config',
                  label: '下载配置',
                  children: (
                    <div className="home-card-body">
                      <div className="home-fields-row">
                        <Form.Item
                          label="文件名"
                          name="filename"
                          className="home-field-item home-field-grow"
                        >
                          <Input placeholder="默认取 URL 末段" allowClear />
                        </Form.Item>
                        <Form.Item
                          label="并发数"
                          name="concurrency"
                          className="home-field-item home-field-concurrency"
                        >
                          <InputNumber min={1} max={64} style={{ width: '100%' }} />
                        </Form.Item>
                      </div>

                      <Form.Item
                        label="输出目录"
                        name="outputDir"
                        className="home-field-item"
                        style={{ marginBottom: 0 }}
                      >
                        <OutputDirField />
                      </Form.Item>

                      <div className="home-prefs">
                        <Form.Item
                          label="合并后清理临时分片"
                          name="cleanupSegments"
                          valuePropName="checked"
                          className="home-pref-item"
                          layout="horizontal"
                          colon={false}
                        >
                          <Switch size="small" />
                        </Form.Item>
                        <Form.Item
                          label="开始后跳转任务列表"
                          name="jumpToTasks"
                          valuePropName="checked"
                          className="home-pref-item"
                          layout="horizontal"
                          colon={false}
                        >
                          <Switch
                            size="small"
                            onChange={(checked) => writeJumpPref(checked)}
                          />
                        </Form.Item>
                      </div>
                    </div>
                  ),
                },
              ]}
            />
          </div>
        </div>

        <Modal
          title="高级选项"
          open={advancedOpen}
          onCancel={() => setAdvancedOpen(false)}
          onOk={() => setAdvancedOpen(false)}
          okText="确定"
          cancelText="取消"
          width={640}
          destroyOnHidden={false}
          styles={{ body: { paddingTop: 8 } }}
          className="advanced-modal"
        >
          <Typography.Paragraph type="secondary" style={{ marginBottom: 16 }}>
            请求相关参数均可选，留空使用默认
          </Typography.Paragraph>
          <Row gutter={[16, 0]}>
            <Col span={12}>
              <Form.Item label="Referer" name="referer">
                <Input allowClear />
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item label="HTTP(S) 代理" name="proxy">
                <Input placeholder="http://127.0.0.1:7890" allowClear />
              </Form.Item>
            </Col>
            <Col span={24}>
              <Form.Item label="Cookie" name="cookie">
                <Input.TextArea rows={2} allowClear />
              </Form.Item>
            </Col>
            <Col span={24}>
              <Form.Item
                label="自定义 Headers"
                name="headersText"
                extra="每行 Key: Value"
                style={{ marginBottom: 0 }}
              >
                <Input.TextArea
                  rows={3}
                  placeholder={'User-Agent: ...\nOrigin: ...'}
                  allowClear
                />
              </Form.Item>
            </Col>
          </Row>
        </Modal>
      </Form>

      <Modal
        title="选择清晰度"
        open={!!variants}
        onCancel={() => {
          setVariants(null)
          setPending(null)
          setLoading(false)
        }}
        onOk={async () => {
          if (!pending || !selectedVariant) return
          setVariants(null)
          await submit(pending, selectedVariant)
          setPending(null)
        }}
        okText="下载所选"
        cancelText="取消"
        destroyOnHidden
      >
        <Typography.Paragraph type="secondary">
          检测到主播放列表，请选择一路媒体流：
        </Typography.Paragraph>
        <Radio.Group
          value={selectedVariant}
          onChange={(e) => setSelectedVariant(e.target.value)}
          style={{ display: 'flex', flexDirection: 'column', gap: 8 }}
        >
          {variants?.map((v) => (
            <Radio key={v.url} value={v.url}>
              {variantLabel(v)}
            </Radio>
          ))}
        </Radio.Group>
      </Modal>
    </>
  )
}

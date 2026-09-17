import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { ConfigProvider } from 'antd'
import zhCN from 'antd/locale/zh_CN'
import App from './App.tsx'
import './index.css'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ConfigProvider
      locale={zhCN}
      theme={{
        token: {
          colorPrimary: '#1677ff',
          colorSuccess: '#52c41a',
          colorWarning: '#faad14',
          colorError: '#ff4d4f',
          borderRadius: 0,
          borderRadiusLG: 0,
          borderRadiusSM: 0,
          borderRadiusXS: 0,
        },
        components: {
          Button: { borderRadius: 0 },
          Input: { borderRadius: 0 },
          Card: { borderRadius: 0 },
          Collapse: { borderRadius: 0 },
          Modal: { borderRadius: 0 },
          Tag: { borderRadius: 0 },
          Progress: { borderRadius: 0 },
          Menu: { borderRadius: 0, itemBorderRadius: 0 },
          Layout: { bodyBg: '#f5f5f5', siderBg: '#ffffff' },
        },
      }}
    >
      <App />
    </ConfigProvider>
  </StrictMode>,
)

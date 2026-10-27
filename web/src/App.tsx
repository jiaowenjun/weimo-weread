import { Popover, Button, Form, Input, App as AntApp } from 'antd';
import { SettingOutlined } from '@ant-design/icons';
import { ProLayout } from '@ant-design/pro-components';
import {
  BrowserRouter,
  Navigate,
  Outlet,
  Route,
  Routes,
  useLocation,
  useNavigate,
} from 'react-router-dom';
import Overview from './pages/Overview';
import Shelf from './pages/Shelf';
import Notes from './pages/Notes';
import Exports from './pages/Exports';
import { getApiBase, getWereadKey, setApiBase, setWereadKey } from './api';

/** 右上角连接设置：API 地址 + 微信读书 API Key（BYOK，必填） */
function ConnectionSettings() {
  const { message } = AntApp.useApp();
  const [form] = Form.useForm<{ apiBase: string; wereadKey: string }>();
  const save = (v: { apiBase: string; wereadKey: string }) => {
    const key = v.wereadKey.trim();
    if (!key.startsWith('wrk-')) {
      message.warning('官方 Key 以 wrk- 开头，请确认粘贴完整（不阻断保存）');
    }
    setApiBase(v.apiBase.trim().replace(/\/$/, ''));
    setWereadKey(key);
    message.success('已保存，正在刷新…');
    window.location.reload();
  };
  return (
    <div style={{ width: 320, padding: 4 }}>
      <Form
        form={form}
        layout="vertical"
        onFinish={save}
        initialValues={{ apiBase: getApiBase(), wereadKey: getWereadKey() }}
      >
        <Form.Item
          name="apiBase"
          label="API 地址"
          rules={[{ required: true }]}
          extra="同源部署保持 /api/v1；跨域填完整地址并需服务器开启 CORS"
        >
          <Input placeholder="/api/v1" />
        </Form.Item>
        <Form.Item
          name="wereadKey"
          label="微信读书 API Key"
          rules={[{ required: true, message: '必填：Key 经浏览器逐请求传给后端，服务器不保存' }]}
          extra="获取页面: https://weread.qq.com/r/weread-skills（仅存本浏览器 localStorage）"
        >
          <Input.Password placeholder="wrk-..." autoComplete="off" />
        </Form.Item>
        <Button type="primary" htmlType="submit" block>
          保存并刷新
        </Button>
      </Form>
    </div>
  );
}

function Layout() {
  const location = useLocation();
  const navigate = useNavigate();
  return (
    <ProLayout
      title="微信读书笔记后台"
      logo={false}
      layout="side"
      fixSiderbar
      location={{ pathname: location.pathname }}
      route={{
        path: '/',
        routes: [
          { path: '/', name: '概览' },
          { path: '/shelf', name: '书架' },
          { path: '/notes', name: '笔记' },
          { path: '/exports', name: '导出任务' },
        ],
      }}
      menuItemRender={(item, dom) => (
        <div onClick={() => item.path && navigate(item.path)}>{dom}</div>
      )}
      actionsRender={() => [
        <Popover key="conn" trigger="click" placement="bottomRight" content={<ConnectionSettings />}>
          <Button type="text" icon={<SettingOutlined />}>
            连接
          </Button>
        </Popover>,
      ]}
    >
      <Outlet />
    </ProLayout>
  );
}

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route element={<Layout />}>
          <Route index element={<Overview />} />
          <Route path="shelf" element={<Shelf />} />
          <Route path="notes" element={<Notes />} />
          <Route path="exports" element={<Exports />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}

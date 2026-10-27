import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Alert, Badge, Button, Card, Col, Empty, Progress, Row, Statistic, Table, Tooltip, theme } from 'antd';
import { PageContainer, ProCard } from '@ant-design/pro-components';
import { ReloadOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import type { ReadData, ServerInfo, ShelfData, TaskSummary } from '../types';
import { api, health } from '../api';
import { TaskStatusTag, fmtDuration } from '../labels';

/** 从 readdata 的宽松结构里按别名取第一个存在的数值字段 */
function pickNum(data: ReadData | null, keys: string[]): number | null {
  if (!data) return null;
  for (const k of keys) {
    const v = data[k];
    if (typeof v === 'number' && v > 0) return v;
  }
  return null;
}

function pickStr(data: ReadData | null, keys: string[]): string | null {
  if (!data) return null;
  for (const k of keys) {
    const v = data[k];
    if (typeof v === 'string' && v.trim()) return v;
    if (Array.isArray(v) && v.length > 0 && typeof v[0] === 'string') return v.slice(0, 3).join('、');
  }
  return null;
}

export default function Overview() {
  const navigate = useNavigate();
  const { token } = theme.useToken();
  const [info, setInfo] = useState<ServerInfo | null>(null);
  const [online, setOnline] = useState<boolean | null>(null);
  const [connError, setConnError] = useState<string | null>(null);
  const [recent, setRecent] = useState<TaskSummary[]>([]);
  const [readdata, setReaddata] = useState<ReadData | null>(null);
  const [shelf, setShelf] = useState<ShelfData | null>(null);

  const load = async () => {
    try {
      await health();
      setOnline(true);
      setConnError(null);
    } catch (e) {
      setOnline(false);
      setConnError((e as Error).message);
      return;
    }
    try {
      setInfo(await api.serverInfo());
    } catch (e) {
      // BYOK：未配置微信读书 Key 时给出明确引导，而非静默空卡片
      if ((e as { code?: string }).code === 'UNAUTHORIZED') {
        setConnError('未配置微信读书 API Key：点击右上角「连接」填写（获取: https://weread.qq.com/r/weread-skills）');
      }
    }
    try {
      const res = await api.listTasks({ limit: 5 });
      setRecent(res.tasks);
    } catch {
      setRecent([]);
    }
    try {
      setReaddata((await api.getReaddata('overall')).data);
    } catch {
      setReaddata(null);
    }
    try {
      setShelf((await api.getShelf()).data);
    } catch {
      setShelf(null);
    }
  };

  useEffect(() => {
    load();
    const timer = setInterval(load, 10_000);
    return () => clearInterval(timer);
  }, []);

  const circuitOpen = info?.circuit.open ?? false;
  const circuitRemainMin = info ? Math.ceil(info.circuit.remaining_ms / 60000) : 0;

  const readingNow = (shelf?.books ?? [])
    .filter((b) => (b.finishReading ?? 0) === 0 && (b.progress ?? 0) > 0)
    .sort((a, b) => (b.readUpdateTime ?? 0) - (a.readUpdateTime ?? 0))
    .slice(0, 6);

  const totalHours = pickNum(readdata, ['totalReadTime', 'readTime', 'totalTime']);

  return (
    <PageContainer
      title="概览"
      subTitle={info ? `${info.name} v${info.version}` : undefined}
      extra={[
        <Button key="refresh" icon={<ReloadOutlined />} onClick={load}>
          刷新
        </Button>,
      ]}
    >
      {connError && (
        <Alert
          type="error"
          showIcon
          style={{ marginBottom: 16 }}
          message="无法连接服务器"
          description={`请确认服务器已启动（默认 127.0.0.1:8081，需配置 WEREAD_API_KEY），或点击右上角「连接」修改 API 地址。错误：${connError}`}
        />
      )}
      {circuitOpen && (
        <Alert
          type="warning"
          showIcon
          style={{ marginBottom: 16 }}
          message={`网关抓取已熔断，预计 ${circuitRemainMin} 分钟后自动恢复。熔断期间新任务继续排队。`}
        />
      )}

      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} lg={6}>
          <Card size="small">
            <Statistic
              title="服务状态"
              valueRender={() =>
                online ? (
                  <Badge status="success" text="运行中" />
                ) : (
                  <Badge status="error" text="不可达" />
                )
              }
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card size="small">
            <Statistic
              title="队列（导出中 / 排队书）"
              value={info ? `${info.queue.running_items} / ${info.queue.waiting}` : '—'}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card size="small">
            <Statistic title="已导出书数" value={info ? info.queue.exported_books : '—'} />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card size="small">
            <Statistic
              title="熔断器"
              valueRender={() =>
                info ? (
                  circuitOpen ? (
                    <Badge status="warning" text={`熔断中 · 剩余 ${circuitRemainMin} 分钟`} />
                  ) : (
                    <Badge status="success" text="正常" />
                  )
                ) : (
                  <>—</>
                )
              }
            />
          </Card>
        </Col>
      </Row>

      <ProCard title="阅读统计（整体）" size="small" style={{ marginTop: 16 }} headerBordered extra={
        <Tooltip title="数据来自官方网关 /readdata/detail，缓存 1 小时">
          <span style={{ color: token.colorTextDescription }}>官方统计</span>
        </Tooltip>
      }>
        {readdata ? (
          <Row gutter={[16, 8]}>
            <Col xs={12} sm={6}>
              <Statistic title="累计阅读时长" value={totalHours != null ? fmtDuration(totalHours) : '—'} />
            </Col>
            <Col xs={12} sm={6}>
              <Statistic
                title="阅读天数"
                value={pickNum(readdata, ['readDays', 'totalReadDays', 'days']) ?? '—'}
              />
            </Col>
            <Col xs={12} sm={6}>
              <Statistic
                title="读完书数"
                value={pickNum(readdata, ['finishBookCount', 'finishedBookCount', 'bookCount']) ?? '—'}
              />
            </Col>
            <Col xs={12} sm={6}>
              <Statistic
                title="最爱作者"
                valueRender={() => (
                  <span style={{ fontSize: 20 }}>
                    {pickStr(readdata, ['preferAuthor', 'favoriteAuthor', 'authorName']) ?? '—'}
                  </span>
                )}
              />
            </Col>
          </Row>
        ) : (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="统计暂不可用" />
        )}
      </ProCard>

      <ProCard
        title="最近在读"
        size="small"
        style={{ marginTop: 16 }}
        headerBordered
        extra={
          <Button type="link" size="small" onClick={() => navigate('/shelf')}>
            查看书架
          </Button>
        }
      >
        {readingNow.length > 0 ? (
          <Row gutter={[12, 12]}>
            {readingNow.map((b) => (
              <Col key={b.bookId} xs={12} sm={8} lg={4}>
                <Card
                  size="small"
                  hoverable
                  onClick={() => navigate(`/shelf?book=${encodeURIComponent(b.bookId)}`)}
                  cover={
                    b.cover ? (
                      <img src={b.cover} alt={b.title} style={{ height: 130, objectFit: 'cover' }} />
                    ) : undefined
                  }
                >
                  <Card.Meta
                    title={<span style={{ fontSize: 13 }}>{b.title}</span>}
                    description={
                      <div style={{ fontSize: 12 }}>
                        <Progress percent={b.progress ?? 0} size="small" showInfo={false} />
                        {b.author}
                      </div>
                    }
                  />
                </Card>
              </Col>
            ))}
          </Row>
        ) : (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="暂无在读书籍" />
        )}
      </ProCard>

      <ProCard
        title="最近导出任务"
        size="small"
        style={{ marginTop: 16 }}
        headerBordered
        extra={
          <Button type="link" size="small" onClick={() => navigate('/exports')}>
            查看全部
          </Button>
        }
      >
        <Table<TaskSummary>
          size="small"
          rowKey="task_id"
          pagination={false}
          dataSource={recent}
          onRow={(r) => ({ onClick: () => navigate(`/exports?task=${r.task_id}`), style: { cursor: 'pointer' } })}
          columns={[
            { title: '任务 ID', dataIndex: 'task_id', ellipsis: true },
            { title: '状态', dataIndex: 'status', width: 100, render: (_, r) => <TaskStatusTag status={r.status} /> },
            { title: '条目（划线+想法）', dataIndex: 'items_count', width: 140 },
            {
              title: '创建时间',
              dataIndex: 'created_at',
              width: 170,
              render: (_, r) => dayjs(r.created_at).format('YYYY-MM-DD HH:mm:ss'),
            },
          ]}
        />
      </ProCard>
    </PageContainer>
  );
}

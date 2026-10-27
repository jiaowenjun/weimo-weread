import { useEffect, useState } from 'react';
import { Alert, Card, Descriptions, Drawer, Progress, Table } from 'antd';
import dayjs from 'dayjs';
import type { ResultView, TaskView } from '../types';
import { api } from '../api';
import { BookStateTag, TaskStatusTag } from '../labels';

const TERMINAL = ['completed', 'partial', 'failed'];

export default function TaskDetailDrawer({
  taskId,
  onClose,
}: {
  taskId: string | null;
  onClose: () => void;
}) {
  const [task, setTask] = useState<TaskView | null>(null);
  const [result, setResult] = useState<ResultView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  // 打开期间：先拉一次快照；未终态则订阅 SSE 推流（进度变更即达），
  // 连接失败（代理不支持流式/网络中断）自动降级为 2s 轮询；终态后拉取结果
  useEffect(() => {
    if (!taskId) {
      setTask(null);
      setResult(null);
      setLoadError(null);
      return;
    }
    let stopped = false;
    let timer: number | undefined;
    let es: EventSource | undefined;

    const fetchResult = async () => {
      try {
        setResult(await api.getResult(taskId));
      } catch (e) {
        if (!stopped) setLoadError(`结果加载失败：${(e as Error).message}`);
      }
    };

    /** 应用任务视图；返回是否终态 */
    const applyTask = (t: TaskView): boolean => {
      if (stopped) return true;
      setTask(t);
      setLoadError(null);
      if (TERMINAL.includes(t.status)) {
        void fetchResult();
        return true;
      }
      return false;
    };

    const startPolling = () => {
      const tick = async () => {
        let terminal = false;
        try {
          terminal = applyTask(await api.getTask(taskId));
        } catch (e) {
          if (!stopped) setLoadError((e as Error).message);
        }
        if (!terminal && !stopped) timer = window.setTimeout(tick, 2000);
      };
      void tick();
    };

    (async () => {
      try {
        if (applyTask(await api.getTask(taskId))) return; // 打开时已是终态
      } catch (e) {
        if (stopped) return;
        setLoadError((e as Error).message);
        timer = window.setTimeout(startPolling, 3000);
        return;
      }

      es = new EventSource(api.taskEventsUrl(taskId));
      const onDelta = (ev: Event) => {
        try {
          applyTask(JSON.parse((ev as MessageEvent).data) as TaskView);
        } catch {
          /* 忽略无法解析的数据帧 */
        }
      };
      es.addEventListener('progress', onDelta);
      es.addEventListener('done', (ev) => {
        onDelta(ev);
        es?.close();
      });
      es.addEventListener('error', (ev) => {
        // 两类 error 事件：业务终态失败（有 data）与连接错误（无 data，EventSource 会自动重连）
        if ((ev as MessageEvent).data != null) {
          onDelta(ev);
          es?.close();
          return;
        }
        // 连接不可用：关闭并降级轮询，避免 EventSource 无限重连
        es?.close();
        es = undefined;
        if (!stopped) startPolling();
      });
    })();

    return () => {
      stopped = true;
      es?.close();
      if (timer) clearTimeout(timer);
    };
  }, [taskId]);

  return (
    <Drawer open={!!taskId} onClose={onClose} width={560} title={task ? `任务 ${task.task_id}` : '任务详情'}>
      {loadError && <Alert type="error" showIcon style={{ marginBottom: 16 }} message={loadError} />}
      {!task && !loadError && <span>加载中…</span>}
      {task && (
        <>
          <Descriptions size="small" column={2} bordered style={{ marginBottom: 16 }}>
            <Descriptions.Item label="状态">
              <TaskStatusTag status={task.status} />
            </Descriptions.Item>
            <Descriptions.Item label="总进度">{task.percent == null ? '—' : `${task.percent}%`}</Descriptions.Item>
            <Descriptions.Item label="书（完成/总数）">
              {task.progress.done_books} / {task.progress.total_books}
            </Descriptions.Item>
            <Descriptions.Item label="已同步条目（划线+想法）">{task.items_count}</Descriptions.Item>
            <Descriptions.Item label="创建时间" span={2}>
              {dayjs(task.created_at).format('YYYY-MM-DD HH:mm:ss')}
            </Descriptions.Item>
            {task.progress.current && (
              <Descriptions.Item label="当前书籍" span={2}>
                {task.progress.current.title ?? task.progress.current.book_id}
              </Descriptions.Item>
            )}
          </Descriptions>

          <Progress
            percent={task.percent ?? 0}
            status={task.status === 'failed' ? 'exception' : TERMINAL.includes(task.status) ? 'success' : 'active'}
            style={{ marginBottom: 16 }}
            format={(p) => (p == null ? '未知' : `${p}%（${task.progress.done_books}/${task.progress.total_books} 本）`)}
          />

          {task.error && (
            <Alert
              type={task.status === 'partial' ? 'warning' : 'error'}
              showIcon
              style={{ marginBottom: 16 }}
              message={`${task.error.code}：${task.error.message}`}
              description={
                task.error.failed_books?.length
                  ? `失败书籍：${task.error.failed_books.map((b) => b.book_id).join('、')}`
                  : undefined
              }
            />
          )}

          {result && (
            <Card size="small" title="同步结果">
              <Descriptions size="small" column={1} style={{ marginBottom: 8 }}>
                <Descriptions.Item label="汇总">
                  {result.summary.succeeded} 本成功
                  {result.summary.failed > 0 ? ` · ${result.summary.failed} 本失败` : ''} · 划线{' '}
                  {result.summary.bookmarks} 条 · 想法 {result.summary.reviews} 条
                </Descriptions.Item>
              </Descriptions>
              <Table<ResultView['books'][number]>
                size="small"
                rowKey="book_id"
                pagination={false}
                dataSource={result.books}
                columns={[
                  { title: '书名', dataIndex: 'title', ellipsis: true, render: (_, r) => r.title ?? r.book_id },
                  { title: '状态', dataIndex: 'state', width: 90, render: (_, r) => <BookStateTag state={r.state} /> },
                  { title: '划线', dataIndex: 'bookmarks', width: 70 },
                  { title: '想法', dataIndex: 'reviews', width: 70 },
                ]}
              />
            </Card>
          )}
        </>
      )}
    </Drawer>
  );
}

import { Tag } from 'antd';

export const TASK_STATUS_META: Record<string, { label: string; color: string }> = {
  queued: { label: '排队中', color: 'default' },
  running: { label: '保存中', color: 'processing' },
  completed: { label: '已完成', color: 'success' },
  partial: { label: '部分成功', color: 'warning' },
  failed: { label: '失败', color: 'error' },
};

export const TASK_STATUSES = Object.keys(TASK_STATUS_META);

export function TaskStatusTag({ status }: { status: string }) {
  const meta = TASK_STATUS_META[status];
  return <Tag color={meta?.color}>{meta?.label ?? status}</Tag>;
}

export function BookStateTag({ state }: { state: string }) {
  const meta: Record<string, { label: string; color: string }> = {
    waiting: { label: '排队', color: 'default' },
    running: { label: '保存中', color: 'processing' },
    done: { label: '完成', color: 'success' },
    skipped: { label: '已缓存', color: 'cyan' },
    error: { label: '失败', color: 'error' },
  };
  const m = meta[state];
  return <Tag color={m?.color}>{m?.label ?? state}</Tag>;
}

/** weread 时间戳（秒或毫秒）→ dayjs 可用值 */
export function wereadTime(v: unknown): number | undefined {
  if (typeof v === 'number' && v > 0) return v > 1_000_000_000_000 ? v : v * 1000;
  return undefined;
}

/** 秒 → "X 小时 Y 分钟" */
export function fmtDuration(seconds: unknown): string {
  if (typeof seconds !== 'number' || seconds <= 0) return '—';
  const h = Math.floor(seconds / 3600);
  const m = Math.round((seconds % 3600) / 60);
  if (h > 0) return m > 0 ? `${h} 小时 ${m} 分` : `${h} 小时`;
  return `${Math.max(m, 1)} 分钟`;
}

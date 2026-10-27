import type {
  BookNotesData,
  CachedEnvelope,
  CreateTaskResponse,
  NotebooksData,
  ReadData,
  ResultView,
  ServerInfo,
  ShelfData,
  TaskListResponse,
  TaskView,
} from './types';

// 连接配置存 localStorage：默认同源 /api/v1（开发期由 Vite 代理），
// 也可填完整地址（如 http://192.168.1.10:8081/api/v1，需服务器开 CORS）。
// BYOK：微信读书 API Key 由用户手动输入，逐请求经 X-Weread-Key 头传给后端。
const BASE_KEY = 'weread-admin:api-base';
const WEREAD_KEY = 'weread-admin:weread-key';

export const getApiBase = () => localStorage.getItem(BASE_KEY) || '/api/v1';
export const setApiBase = (v: string) => localStorage.setItem(BASE_KEY, v);
export const getWereadKey = () => localStorage.getItem(WEREAD_KEY) || '';
export const setWereadKey = (v: string) => localStorage.setItem(WEREAD_KEY, v);

export class ApiError extends Error {
  code: string;
  status: number;
  constructor(message: string, code: string, status: number) {
    super(message);
    this.code = code;
    this.status = status;
  }
}

async function parseError(res: Response): Promise<ApiError> {
  let code = 'HTTP_ERROR';
  let message = `请求失败（HTTP ${res.status}）`;
  try {
    const body = await res.json();
    if (body?.error?.code) code = body.error.code;
    if (body?.error?.message) message = body.error.message;
  } catch {
    /* 响应体不是 JSON（如网关错误页）时保留默认信息 */
  }
  return new ApiError(message, code, res.status);
}

function authHeaders(): Record<string, string> {
  const key = getWereadKey();
  return key ? { 'X-Weread-Key': key } : {};
}

export async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const res = await fetch(getApiBase() + path, {
    ...init,
    headers: {
      ...authHeaders(),
      ...(init.body ? { 'Content-Type': 'application/json' } : {}),
    },
  });
  if (!res.ok) throw await parseError(res);
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

/** 带 refresh 参数的代理端点请求 */
function withRefresh(path: string, refresh?: boolean) {
  return path + (refresh ? '?refresh=true' : '');
}

/** /healthz 不在 /api/v1 前缀下且无需 Key（探活开放） */
export const health = async () => {
  const origin = getApiBase().replace(/\/api\/v1\/?$/, '').replace(/\/$/, '');
  const res = await fetch(origin + '/healthz');
  if (!res.ok) throw await parseError(res);
  return (await res.json()) as { status: string };
};

export const api = {
  serverInfo: () => request<ServerInfo>('/server/info'),

  // ---- 同步代理 ----
  getShelf: (refresh?: boolean) =>
    request<CachedEnvelope<ShelfData>>(withRefresh('/shelf', refresh)),
  getNotebooks: (refresh?: boolean) =>
    request<CachedEnvelope<NotebooksData>>(withRefresh('/notebooks', refresh)),
  getBookNotes: (bookId: string, refresh?: boolean) =>
    request<CachedEnvelope<BookNotesData>>(
      withRefresh(`/books/${encodeURIComponent(bookId)}/notes`, refresh),
    ),
  getReaddata: (mode: string, refresh?: boolean, baseTime?: number) => {
    const qs = new URLSearchParams({ mode });
    if (baseTime != null) qs.set('baseTime', String(baseTime));
    if (refresh) qs.set('refresh', 'true');
    return request<CachedEnvelope<ReadData>>(`/readdata?${qs.toString()}`);
  },

  // ---- 同步任务 ----
  listTasks: (query: { status?: string; limit?: number; cursor?: number }) => {
    const qs = new URLSearchParams();
    if (query.status) qs.set('status', query.status);
    qs.set('limit', String(query.limit ?? 100));
    if (query.cursor != null) qs.set('cursor', String(query.cursor));
    return request<TaskListResponse>(`/syncs?${qs.toString()}`);
  },

  getTask: (taskId: string) => request<TaskView>(`/syncs/${taskId}`),

  /** SSE 订阅地址（EventSource 无法设置 Header，Key 经 ?key= 查询参数传递） */
  taskEventsUrl: (taskId: string) => {
    const key = getWereadKey();
    const q = key ? `?key=${encodeURIComponent(key)}` : '';
    return `${getApiBase()}/syncs/${encodeURIComponent(taskId)}/events${q}`;
  },

  createTask: (body: { book_ids?: string[] | null; refresh?: boolean; wait_timeout?: number }) =>
    request<CreateTaskResponse>('/syncs', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  deleteTask: (taskId: string) =>
    request<void>(`/syncs/${taskId}`, { method: 'DELETE' }),

  getResult: (taskId: string) => request<ResultView>(`/syncs/${taskId}/result`),
};

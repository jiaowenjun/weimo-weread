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
// 也可填完整地址（如 http://192.168.1.10:8081/api/v1，需服务器开 CORS）
const BASE_KEY = 'weread-admin:api-base';
const TOKEN_KEY = 'weread-admin:token';

export const getApiBase = () => localStorage.getItem(BASE_KEY) || '/api/v1';
export const setApiBase = (v: string) => localStorage.setItem(BASE_KEY, v);
export const getToken = () => localStorage.getItem(TOKEN_KEY) || '';
export const setToken = (v: string) => localStorage.setItem(TOKEN_KEY, v);

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
  const token = getToken();
  return token ? { Authorization: `Bearer ${token}` } : {};
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

/** /healthz 不在 /api/v1 前缀下，不能走 request()（它会再拼一次 base） */
export const health = async () => {
  const origin = getApiBase().replace(/\/api\/v1\/?$/, '').replace(/\/$/, '');
  const res = await fetch(origin + '/healthz', { headers: authHeaders() });
  if (!res.ok) throw await parseError(res);
  return (await res.json()) as { status: string };
};

/** Markdown 直链（<a download> 无法带 Header，token 经 ?token= 传递，服务端支持） */
export const markdownUrl = (bookId: string, download = false) => {
  const qs = new URLSearchParams();
  if (download) qs.set('download', 'true');
  const token = getToken();
  if (token) qs.set('token', token);
  const q = qs.toString();
  return `${getApiBase()}/books/${encodeURIComponent(bookId)}/notes/markdown${q ? `?${q}` : ''}`;
};

/** 探测某书是否已导出过 Markdown（只看状态码，不下载正文） */
export const markdownExists = async (bookId: string): Promise<boolean> => {
  const res = await fetch(markdownUrl(bookId), { headers: authHeaders() });
  const ok = res.ok;
  try {
    await res.body?.cancel();
  } catch {
    /* body 未开时忽略 */
  }
  return ok;
};

export const api = {
  serverInfo: () => request<ServerInfo>('/server/info'),
  markdownExists,

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

  // ---- 导出任务 ----
  listTasks: (query: { status?: string; limit?: number; cursor?: number }) => {
    const qs = new URLSearchParams();
    if (query.status) qs.set('status', query.status);
    qs.set('limit', String(query.limit ?? 100));
    if (query.cursor != null) qs.set('cursor', String(query.cursor));
    return request<TaskListResponse>(`/exports?${qs.toString()}`);
  },

  getTask: (taskId: string) => request<TaskView>(`/exports/${taskId}`),

  /** SSE 订阅地址（EventSource 无法设置 Header，token 经 ?token= 查询参数传递） */
  taskEventsUrl: (taskId: string) => {
    const token = getToken();
    const q = token ? `?token=${encodeURIComponent(token)}` : '';
    return `${getApiBase()}/exports/${encodeURIComponent(taskId)}/events${q}`;
  },

  createTask: (body: { book_ids?: string[] | null; refresh?: boolean; wait_timeout?: number }) =>
    request<CreateTaskResponse>('/exports', {
      method: 'POST',
      body: JSON.stringify(body),
    }),

  deleteTask: (taskId: string) =>
    request<void>(`/exports/${taskId}`, { method: 'DELETE' }),

  getResult: (taskId: string) => request<ResultView>(`/exports/${taskId}/result`),
};

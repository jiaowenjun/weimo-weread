// 与 src/server/service.rs 的响应结构一一对应

export type TaskStatus = 'queued' | 'running' | 'completed' | 'partial' | 'failed';

/** 同步代理端点的统一信封 */
export interface CachedEnvelope<T> {
  cached_at: string;
  fresh: boolean;
  data: T;
}

// ---- 书架 / 笔记本 ----

export interface ShelfBook {
  bookId: string;
  title: string;
  author?: string;
  cover?: string;
  progress?: number;
  finishReading?: number;
  readUpdateTime?: number;
  [k: string]: unknown;
}

export interface ShelfData {
  books: ShelfBook[];
  albums?: { bookId?: string; title?: string }[];
  archive?: { archiveId?: number; name?: string }[];
  [k: string]: unknown;
}

export interface NotebookItem {
  bookId: string;
  bookmarkCount: number;
  noteCount: number;
  reviewCount: number;
  updateTime?: number | null;
  readingProgress?: number;
  book: { title?: string; author?: string; cover?: string; [k: string]: unknown };
}

export interface NotebooksData {
  books: NotebookItem[];
  totalBookCount: number | null;
  totalNoteCount: number | null;
}

// ---- 单书笔记 ----

export interface Chapter {
  chapterUid: number;
  title: string;
}

export interface BookmarkItem {
  chapterUid: number;
  markText: string;
  createTime: number;
  [k: string]: unknown;
}

export interface ReviewItem {
  reviewId: string;
  review: {
    content?: string;
    abstract?: string;
    createTime?: number;
    chapterUid?: number;
    [k: string]: unknown;
  };
  [k: string]: unknown;
}

export interface BookNotesData {
  bookId: string;
  book: { title?: string; author?: string; cover?: string; [k: string]: unknown };
  chapters: Chapter[];
  bookmarks: BookmarkItem[];
  reviews: ReviewItem[];
}

// ---- 阅读统计（字段多且官方未列全 schema，宽松索引） ----

export interface ReadData {
  [k: string]: unknown;
}

// ---- 任务 ----

export interface TaskProgress {
  done_books: number;
  total_books: number;
  skipped_books: number;
  failed_books: number;
  current: { book_id: string; title: string | null } | null;
}

export interface TaskError {
  code: string;
  message: string;
  failed_books?: { book_id: string; error?: string }[];
}

export interface TaskView {
  task_id: string;
  status: TaskStatus;
  request: { book_ids?: string[]; explicit_books?: boolean; refresh?: boolean };
  progress: TaskProgress;
  items_count: number;
  error: TaskError | null;
  percent: number | null;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  expires_at: string | null;
  result_url: string;
}

export interface CreateTaskResponse extends TaskView {
  skipped_cached?: number;
  queued_books?: number;
}

export interface TaskSummary {
  task_id: string;
  status: TaskStatus;
  items_count: number;
  created_at: string;
  url: string;
}

export interface TaskListResponse {
  tasks: TaskSummary[];
  next_cursor: number | null;
  has_more: boolean;
}

export interface ResultBook {
  book_id: string;
  title: string | null;
  state: string;
  bookmarks: number | null;
  reviews: number | null;
  error: string | null;
  markdown_url: string | null;
}

export interface ResultView {
  task_id: string;
  status: TaskStatus;
  summary: {
    books: number;
    succeeded: number;
    failed: number;
    bookmarks: number;
    reviews: number;
  };
  books: ResultBook[];
}

// ---- 服务元信息 ----

export interface ServerInfo {
  name: string;
  version: string;
  key_mode: string;
  gateway: {
    base_url: string;
    skill_version: string;
    min_interval_ms: number;
  };
  cache: { ttl_seconds: number };
  queue: {
    waiting: number;
    running_items: number;
    running_tasks: number;
    exported_books: number;
  };
  circuit: {
    threshold: number;
    cooldown_seconds: number;
    open: boolean;
    remaining_ms: number;
  };
}

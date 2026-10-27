import { useCallback, useEffect, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { Button, Drawer, Progress, Spin, Table, Tag, Collapse, Descriptions, Empty } from 'antd';
import { PageContainer, ProCard } from '@ant-design/pro-components';
import { ReloadOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import type { CachedEnvelope, ShelfBook, ShelfData } from '../types';
import { api, request } from '../api';
import { wereadTime } from '../labels';

type BookInfoEnvelope = CachedEnvelope<Record<string, unknown>>;
type ChaptersEnvelope = CachedEnvelope<{ chapters?: { chapterUid: number; title: string }[] }>;

function BookDetailDrawer({ bookId, onClose }: { bookId: string | null; onClose: () => void }) {
  const [info, setInfo] = useState<BookInfoEnvelope | null>(null);
  const [chapters, setChapters] = useState<ChaptersEnvelope | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!bookId) {
      setInfo(null);
      setChapters(null);
      setError(null);
      return;
    }
    setLoading(true);
    (async () => {
      try {
        const [i, c] = await Promise.all([
          request<BookInfoEnvelope>(`/books/${encodeURIComponent(bookId)}`),
          request<ChaptersEnvelope>(`/books/${encodeURIComponent(bookId)}/chapters`),
        ]);
        setInfo(i);
        setChapters(c);
        setError(null);
      } catch (e) {
        setError((e as Error).message);
      } finally {
        setLoading(false);
      }
    })();
  }, [bookId]);

  const d = info?.data ?? {};
  const str = (k: string) => {
    const v = d[k];
    if (typeof v === 'string' && v.trim()) return v;
    if (typeof v === 'number') return String(v);
    return null;
  };

  return (
    <Drawer open={!!bookId} onClose={onClose} width={480} title={`书籍详情 ${bookId ?? ''}`}>
      {loading && <Spin />}
      {error && <div style={{ color: '#ff4d4f' }}>{error}</div>}
      {!loading && !error && info && (
        <>
          <Descriptions size="small" column={1} bordered>
            <Descriptions.Item label="书名">{str('title') ?? '—'}</Descriptions.Item>
            <Descriptions.Item label="作者">{str('author') ?? '—'}</Descriptions.Item>
            {str('translator') && <Descriptions.Item label="译者">{str('translator')}</Descriptions.Item>}
            {str('publisher') && <Descriptions.Item label="出版社">{str('publisher')}</Descriptions.Item>}
            {str('isbn') && <Descriptions.Item label="ISBN">{str('isbn')}</Descriptions.Item>}
            {str('wordCount') && <Descriptions.Item label="字数">{str('wordCount')}</Descriptions.Item>}
            {str('newRating') && (
              <Descriptions.Item label="评分">{(Number(d['newRating']) / 10).toFixed(1)}</Descriptions.Item>
            )}
            {str('intro') && (
              <Descriptions.Item label="简介">
                <div style={{ maxHeight: 160, overflow: 'auto', whiteSpace: 'pre-wrap' }}>{str('intro')}</div>
              </Descriptions.Item>
            )}
          </Descriptions>
          <Collapse
            size="small"
            style={{ marginTop: 16 }}
            items={[
              {
                key: 'chapters',
                label: `目录（${chapters?.data?.chapters?.length ?? 0} 章）`,
                children: (
                  <div style={{ maxHeight: 400, overflow: 'auto' }}>
                    {(chapters?.data?.chapters ?? []).map((c) => (
                      <div key={c.chapterUid} style={{ padding: '2px 0', borderBottom: '1px solid #f5f5f5' }}>
                        {c.title}
                      </div>
                    ))}
                  </div>
                ),
              },
            ]}
          />
        </>
      )}
    </Drawer>
  );
}

export default function Shelf() {
  const [searchParams] = useSearchParams();
  const [envelope, setEnvelope] = useState<CachedEnvelope<ShelfData> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [openBook, setOpenBook] = useState<string | null>(null);

  const load = useCallback(async (refresh = false) => {
    setLoading(true);
    try {
      setEnvelope(await api.getShelf(refresh));
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const book = searchParams.get('book');
    if (book) setOpenBook(book);
  }, [searchParams]);

  const books: ShelfBook[] = envelope?.data?.books ?? [];
  const albums = envelope?.data?.albums ?? [];
  const archive = envelope?.data?.archive ?? [];
  const sorted = [...books].sort((a, b) => (b.readUpdateTime ?? 0) - (a.readUpdateTime ?? 0));

  return (
    <PageContainer
      title="书架"
      subTitle={envelope ? `共 ${books.length} 本 · 更新于 ${dayjs(envelope.cached_at).format('HH:mm')}` : undefined}
      extra={[
        <Button key="refresh" icon={<ReloadOutlined />} loading={loading} onClick={() => load(false)}>
          刷新
        </Button>,
        <Button key="force" type="primary" ghost onClick={() => load(true)} loading={loading}>
          强制刷新
        </Button>,
      ]}
    >
      {error && <div style={{ color: '#ff4d4f', marginBottom: 12 }}>{error}</div>}
      {(albums.length > 0 || archive.length > 0) && (
        <ProCard size="small" style={{ marginBottom: 12 }}>
          听书专辑 {albums.length} 个 · 自建书单 {archive.length} 个（v1 仅展示电子书列表）
        </ProCard>
      )}
      <Table<ShelfBook>
        size="small"
        rowKey="bookId"
        loading={loading}
        dataSource={sorted}
        pagination={{ pageSize: 20, showSizeChanger: true, showTotal: (t) => `共 ${t} 本` }}
        onRow={(r) => ({ onClick: () => setOpenBook(r.bookId), style: { cursor: 'pointer' } })}
        columns={[
          {
            title: '封面',
            width: 56,
            render: (_, r) =>
              r.cover ? (
                <img src={r.cover} alt="" style={{ width: 40, height: 56, objectFit: 'cover', borderRadius: 2 }} />
              ) : (
                '—'
              ),
          },
          {
            title: '书名',
            dataIndex: 'title',
            ellipsis: true,
            render: (_, r) => (
              <>
                {r.title}{' '}
                {(r.finishReading ?? 0) === 1 && <Tag color="green">读完</Tag>}
              </>
            ),
          },
          { title: '作者', dataIndex: 'author', width: 160, ellipsis: true, render: (_, r) => r.author ?? '—' },
          {
            title: '进度',
            dataIndex: 'progress',
            width: 180,
            sorter: (a, b) => (a.progress ?? 0) - (b.progress ?? 0),
            render: (_, r) => (
              <Progress
                percent={r.progress ?? 0}
                size="small"
                status={(r.progress ?? 0) >= 100 ? 'success' : 'active'}
              />
            ),
          },
          {
            title: '最近阅读',
            dataIndex: 'readUpdateTime',
            width: 150,
            defaultSortOrder: 'descend',
            sorter: (a, b) => (a.readUpdateTime ?? 0) - (b.readUpdateTime ?? 0),
            render: (_, r) => {
              const ms = wereadTime(r.readUpdateTime);
              return ms ? dayjs(ms).format('YYYY-MM-DD HH:mm') : '—';
            },
          },
        ]}
        locale={{ emptyText: <Empty description="书架为空或尚未同步" /> }}
      />
      <BookDetailDrawer bookId={openBook} onClose={() => setOpenBook(null)} />
    </PageContainer>
  );
}

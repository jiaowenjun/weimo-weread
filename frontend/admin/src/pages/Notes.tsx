import { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { App as AntApp, Button, Card, Col, Empty, Input, List, Row, Spin, Tag, Tooltip, theme } from 'antd';
import { ReloadOutlined, SaveOutlined } from '@ant-design/icons';
import { PageContainer } from '@ant-design/pro-components';
import dayjs from 'dayjs';
import type { BookNotesData, CachedEnvelope, NotebooksData } from '../types';
import { api } from '../api';
import { wereadTime } from '../labels';

function fmtTs(v: unknown): string {
  const ms = wereadTime(v);
  return ms ? dayjs(ms).format('YYYY-MM-DD HH:mm') : '';
}

/** 单书笔记面板：按章节分组渲染划线 + 我的想法，可一键同步到服务端 */
function BookNotesPanel({ bookId, onSynced }: { bookId: string; onSynced?: () => void }) {
  const { message } = AntApp.useApp();
  const { token } = theme.useToken();
  const [envelope, setEnvelope] = useState<CachedEnvelope<BookNotesData> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);

  const load = useCallback(
    async (refresh = false) => {
      setLoading(true);
      try {
        const env = await api.getBookNotes(bookId, refresh);
        setEnvelope(env);
        setError(null);
      } catch (e) {
        setError((e as Error).message);
      } finally {
        setLoading(false);
      }
    },
    [bookId],
  );

  useEffect(() => {
    void load(false);
  }, [bookId, load]);

  const notes = envelope?.data;
  const title = notes?.book?.title ?? bookId;
  const author = notes?.book?.author ?? '';

  // 划线按章节分组（章节顺序对齐官方目录）
  const grouped = useMemo(() => {
    if (!notes) return [];
    const titleByUid = new Map<number, string>();
    for (const c of notes.chapters) titleByUid.set(c.chapterUid, c.title);
    const map = new Map<number, typeof notes.bookmarks>();
    for (const b of notes.bookmarks) {
      const uid = b.chapterUid ?? 0;
      if (!map.has(uid)) map.set(uid, []);
      map.get(uid)!.push(b);
    }
    return [...map.entries()]
      .sort((a, b) => a[0] - b[0])
      .map(([uid, items]) => ({
        uid,
        title: titleByUid.get(uid) ?? (uid > 0 ? `第 ${uid} 章` : '未分章'),
        items: [...items].sort((a, b) => (a.createTime ?? 0) - (b.createTime ?? 0)),
      }));
  }, [notes]);

  const syncBook = async () => {
    setSyncing(true);
    try {
      const res = await api.createTask({ book_ids: [bookId], wait_timeout: 30 });
      if (['completed', 'partial'].includes(res.status)) {
        message.success(`已同步《${title}》到服务端（${res.items_count} 条）`);
        onSynced?.();
      } else {
        message.info('同步仍在进行，请到「同步任务」页查看进度');
      }
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setSyncing(false);
    }
  };

  return (
    <Card
      size="small"
      title={
        <span>
          {title} {author && <span style={{ color: token.colorTextDescription, fontSize: 12 }}>· {author}</span>}
        </span>
      }
      extra={
        <Tooltip title="笔记原始数据（书/章节/划线/想法）同步到服务端 SQLite 长期保留">
          <Button size="small" icon={<SaveOutlined />} loading={syncing} onClick={syncBook}>
            同步此书
          </Button>
        </Tooltip>
      }
    >
      {loading && <Spin style={{ display: 'block', margin: '24px auto' }} />}
      {error && !loading && <div style={{ color: token.colorError }}>{error}</div>}
      {notes && !loading && (
        <div style={{ maxHeight: 'calc(100vh - 210px)', overflow: 'auto', paddingRight: 4 }}>
          {grouped.map((g) => (
            <div key={g.uid} style={{ marginBottom: 16 }}>
              <div style={{ fontWeight: 600, borderBottom: `1px solid ${token.colorSplit}`, paddingBottom: 4, marginBottom: 8 }}>
                {g.title}
                <Tag style={{ marginLeft: 8 }}>{g.items.length} 划线</Tag>
              </div>
              {g.items.map((b, i) => (
                <blockquote
                  key={i}
                  style={{
                    margin: '0 0 8px',
                    padding: '4px 12px',
                    borderLeft: `3px solid ${token.colorBorderSecondary}`,
                    background: token.colorFillQuaternary,
                    color: token.colorText,
                  }}
                >
                  <div>{b.markText}</div>
                  {b.createTime != null && (
                    <div style={{ fontSize: 12, color: token.colorTextTertiary, marginTop: 2 }}>{fmtTs(b.createTime)}</div>
                  )}
                </blockquote>
              ))}
            </div>
          ))}

          {(notes.reviews?.length ?? 0) > 0 && (
            <div>
              <div style={{ fontWeight: 600, borderBottom: `1px solid ${token.colorSplit}`, paddingBottom: 4, marginBottom: 8 }}>
                我的想法
                <Tag style={{ marginLeft: 8 }}>{notes.reviews.length} 条</Tag>
              </div>
              {notes.reviews.map((r) => (
                <div
                  key={r.reviewId}
                  style={{ marginBottom: 10, padding: '6px 12px', background: token.colorSuccessBg, borderRadius: 4 }}
                >
                  <div>{r.review?.content || <span style={{ color: token.colorTextDescription }}>(仅标注)</span>}</div>
                  {r.review?.abstract && (
                    <div style={{ fontSize: 12, color: token.colorTextDescription, marginTop: 4 }}>原文：{r.review.abstract}</div>
                  )}
                  {r.review?.createTime != null && (
                    <div style={{ fontSize: 12, color: token.colorTextTertiary, marginTop: 2 }}>{fmtTs(r.review.createTime)}</div>
                  )}
                </div>
              ))}
            </div>
          )}

          {notes.bookmarks.length === 0 && notes.reviews.length === 0 && (
            <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="这本书没有划线或想法" />
          )}
        </div>
      )}
    </Card>
  );
}

export default function Notes() {
  const navigate = useNavigate();
  const { token } = theme.useToken();
  const [envelope, setEnvelope] = useState<CachedEnvelope<NotebooksData> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [keyword, setKeyword] = useState('');

  const load = useCallback(async (refresh = false) => {
    setLoading(true);
    try {
      setEnvelope(await api.getNotebooks(refresh));
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

  const books = useMemo(() => {
    const list = [...(envelope?.data?.books ?? [])];
    list.sort((a, b) => {
      const ta = wereadTime(a.updateTime) ?? 0;
      const tb = wereadTime(b.updateTime) ?? 0;
      return tb - ta;
    });
    const kw = keyword.trim();
    return kw
      ? list.filter(
          (b) => b.book?.title?.includes(kw) || b.bookId.includes(kw),
        )
      : list;
  }, [envelope, keyword]);

  return (
    <PageContainer
      title="笔记"
      subTitle={
        envelope
          ? `${envelope.data.totalBookCount ?? envelope.data.books.length} 本有笔记 · 共 ${envelope.data.totalNoteCount ?? '—'} 条`
          : undefined
      }
      extra={[
        <Button key="refresh" icon={<ReloadOutlined />} loading={loading} onClick={() => load(false)}>
          刷新
        </Button>,
        <Button key="sync-all" type="primary" onClick={() => navigate('/syncs')}>
          全部同步
        </Button>,
      ]}
    >
      {error && <div style={{ color: token.colorError, marginBottom: 12 }}>{error}</div>}
      <Row gutter={16}>
        <Col xs={24} md={10} lg={8}>
          <Card
            size="small"
            title="有笔记的书"
            extra={
              <Input.Search
                size="small"
                placeholder="搜书名 / bookId"
                style={{ width: 160 }}
                onSearch={setKeyword}
                onChange={(e) => !e.target.value && setKeyword('')}
                allowClear
              />
            }
            styles={{ body: { maxHeight: 'calc(100vh - 210px)', overflow: 'auto', padding: 0 } }}
          >
            <List
              size="small"
              loading={loading}
              dataSource={books}
              locale={{ emptyText: <Empty description="没有做过笔记的书" /> }}
              renderItem={(nb) => (
                <List.Item
                  style={{
                    cursor: 'pointer',
                    padding: '8px 12px',
                    background: selected === nb.bookId ? token.colorPrimaryBg : undefined,
                  }}
                  onClick={() => setSelected(nb.bookId)}
                >
                  <List.Item.Meta
                    avatar={
                      nb.book?.cover ? (
                        <img src={nb.book.cover} alt="" style={{ width: 28, height: 40, objectFit: 'cover' }} />
                      ) : undefined
                    }
                    title={<span style={{ fontSize: 13 }}>{nb.book?.title ?? nb.bookId}</span>}
                    description={
                      <span style={{ fontSize: 12 }}>
                        划线 {nb.bookmarkCount} · 笔记 {nb.noteCount} · 想法 {nb.reviewCount}
                        {nb.updateTime ? ` · ${fmtTs(nb.updateTime)}` : ''}
                      </span>
                    }
                  />
                </List.Item>
              )}
            />
          </Card>
        </Col>
        <Col xs={24} md={14} lg={16}>
          {selected ? (
            <BookNotesPanel key={selected} bookId={selected} />
          ) : (
            <Card size="small">
              <Empty description="从左侧选择一本书查看划线与想法" style={{ margin: '48px 0' }} />
            </Card>
          )}
        </Col>
      </Row>
    </PageContainer>
  );
}

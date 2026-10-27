import { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { App as AntApp, Badge, Button, Card, Col, Empty, Input, List, Row, Spin, Tag, Tooltip, theme } from 'antd';
import { DownloadOutlined, ExportOutlined, ReloadOutlined } from '@ant-design/icons';
import { PageContainer } from '@ant-design/pro-components';
import dayjs from 'dayjs';
import type { BookNotesData, CachedEnvelope, NotebooksData } from '../types';
import { api, markdownUrl } from '../api';
import { wereadTime } from '../labels';

function fmtTs(v: unknown): string {
  const ms = wereadTime(v);
  return ms ? dayjs(ms).format('YYYY-MM-DD HH:mm') : '';
}

/** 单书笔记面板：按章节分组渲染划线 + 我的想法（结构对齐导出的 Markdown） */
function BookNotesPanel({ bookId, onExported }: { bookId: string; onExported?: () => void }) {
  const { message } = AntApp.useApp();
  const { token } = theme.useToken();
  const [envelope, setEnvelope] = useState<CachedEnvelope<BookNotesData> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exported, setExported] = useState<boolean | null>(null);

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
    setExported(null);
    void load(false);
    api
      .markdownExists(bookId)
      .then(setExported)
      .catch(() => setExported(false));
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

  const exportBook = async () => {
    setExporting(true);
    try {
      const res = await api.createTask({ book_ids: [bookId], wait_timeout: 30 });
      if (['completed', 'partial'].includes(res.status)) {
        setExported(true);
        message.success(`已导出《${title}》（${res.items_count} 条）`);
        onExported?.();
      } else {
        message.info('导出仍在进行，请到「导出任务」页查看进度');
      }
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setExporting(false);
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
        <>
          {exported === true && (
            <Tooltip title="导出的 Markdown 长期保存在服务端">
              <Badge status="success" text="已导出" style={{ marginRight: 12 }} />
            </Tooltip>
          )}
          <Button
            size="small"
            icon={<ExportOutlined />}
            loading={exporting}
            onClick={exportBook}
            style={{ marginRight: 8 }}
          >
            导出此书
          </Button>
          {exported && (
            <Button
              size="small"
              type="primary"
              ghost
              icon={<DownloadOutlined />}
              href={markdownUrl(bookId, true)}
              target="_blank"
            >
              下载 Markdown
            </Button>
          )}
        </>
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
        <Button key="export-all" type="primary" onClick={() => navigate('/exports')}>
          全量导出
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

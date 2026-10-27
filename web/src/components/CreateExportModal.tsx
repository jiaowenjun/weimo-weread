import { App as AntApp, Button } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import { ModalForm, ProFormSwitch, ProFormTextArea } from '@ant-design/pro-components';
import { api } from '../api';

export default function CreateExportModal({ onCreated }: { onCreated: (taskId: string) => void }) {
  const { message } = AntApp.useApp();
  return (
    <ModalForm
      title="新建导出任务"
      width={520}
      trigger={
        <Button type="primary" icon={<PlusOutlined />}>
          新建导出
        </Button>
      }
      modalProps={{ destroyOnClose: true }}
      initialValues={{ allBooks: true, refresh: false }}
      onFinish={async (values) => {
        try {
          const bookIds: string[] | undefined = values.allBooks
            ? undefined
            : String(values.bookIds ?? '')
                .split(/[\s,，;；]+/)
                .map((s) => s.trim())
                .filter(Boolean);
          if (!values.allBooks && (!bookIds || bookIds.length === 0)) {
            message.error('请填写至少一个 bookId，或切换为全部书');
            return false;
          }
          const res = await api.createTask({
            book_ids: bookIds ?? null,
            refresh: values.refresh ?? false,
          });
          message.success(
            `任务已创建（${res.skipped_cached ?? 0} 本命中已有导出，${res.queued_books ?? 0} 本排队抓取）`,
          );
          onCreated(res.task_id);
          return true;
        } catch (e) {
          message.error((e as Error).message);
          return false;
        }
      }}
    >
      <ProFormSwitch
        name="allBooks"
        label="导出全部有笔记的书"
        extra="关闭后可手动指定 bookId（笔记页的列表里有）"
      />
      <ProFormTextArea
        name="bookIds"
        label="指定 bookId 列表"
        placeholder={'每行一个或逗号分隔\n如：3300028061, 934835'}
        dependencies={['allBooks']}
      />
      <ProFormSwitch name="refresh" label="强制重新导出" extra="已导出的书也重抓并覆盖（默认跳过）" />
    </ModalForm>
  );
}

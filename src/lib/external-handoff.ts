export type HandoffStatus = 'sent' | 'unknown';

/** A downloader receipt reports a handoff, never progress or a saved file. */
export function handoffMessage(name: string, status: HandoffStatus): string {
  return status === 'sent'
    ? `已交给 ${name}，请在下载器中查看进度。`
    : `未确认 ${name} 是否接收，请先检查下载器队列，避免重复下载。`;
}

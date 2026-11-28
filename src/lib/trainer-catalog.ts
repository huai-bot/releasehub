import { invoke, isTauri } from '@tauri-apps/api/core';

export type TrainerCatalogMode = 'latest' | 'all' | 'search';

export interface TrainerEntry {
  title: string;
  url: string;
  publishedAt: string | null;
  updatedAt: string | null;
  summary: string;
}

export interface TrainerListPage {
  items: TrainerEntry[];
  page: number;
  hasNextPage: boolean;
  totalPages: number | null;
}

export interface TrainerAttachment {
  label: string;
  url: string;
}

export interface TrainerDetail extends TrainerEntry {
  content: string;
  attachments: TrainerAttachment[];
}

export type TrainerSaveResult = { status: 'saved'; path: string } | { status: 'cancelled' };
export type TrainerExternalSendResult = { status: 'sent' | 'unknown' };

type CatalogActions = {
  trainer_catalog_list: { payload: { mode: TrainerCatalogMode; query: string; page: number; forceRefresh?: boolean }; result: TrainerListPage };
  trainer_catalog_detail: { payload: { url: string; forceRefresh?: boolean }; result: TrainerDetail };
  trainer_catalog_save: { payload: { detailUrl: string; attachmentUrl: string }; result: TrainerSaveResult };
  trainer_catalog_send_external: { payload: { detailUrl: string; attachmentUrl: string }; result: TrainerExternalSendResult };
  trainer_catalog_original: { payload: { url: string }; result: null };
  trainer_catalog_reveal: { payload: { path: string }; result: null };
};

async function call<Action extends keyof CatalogActions>(action: Action, payload: CatalogActions[Action]['payload']): Promise<CatalogActions[Action]['result']> {
  if (!isTauri()) throw new Error('请从追新桌面程序打开风灵月影。');
  try {
    return await invoke<CatalogActions[Action]['result']>('dispatch', { action, payload });
  } catch (error) {
    if (typeof error === 'string' && error.trim()) throw new Error(error);
    if (error instanceof Error && error.message.trim()) throw error;
    throw new Error(action === 'trainer_catalog_send_external'
      ? '无法确认发送结果，请先检查下载器任务，不要直接重发。'
      : '风灵月影操作未完成，请重试。');
  }
}

export async function listTrainers(mode: TrainerCatalogMode, query: string, page: number, forceRefresh = false): Promise<TrainerListPage> {
  return call('trainer_catalog_list', { mode, query, page, ...(forceRefresh ? { forceRefresh } : {}) });
}

export async function getTrainerDetail(url: string, forceRefresh = false): Promise<TrainerDetail> {
  return call('trainer_catalog_detail', { url, ...(forceRefresh ? { forceRefresh } : {}) });
}

export async function saveTrainerAttachment(detailUrl: string, attachmentUrl: string): Promise<TrainerSaveResult> {
  return call('trainer_catalog_save', { detailUrl, attachmentUrl });
}

export async function sendTrainerToDownloader(detailUrl: string, attachmentUrl: string): Promise<TrainerExternalSendResult> {
  return call('trainer_catalog_send_external', { detailUrl, attachmentUrl });
}

export async function openTrainerOriginal(url: string): Promise<void> {
  await call('trainer_catalog_original', { url });
}

export async function revealSavedTrainer(path: string): Promise<void> {
  await call('trainer_catalog_reveal', { path });
}

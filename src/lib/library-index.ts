import type { LibrarySort, Project, RepositoryNote } from './types';
import { hasProjectUpdate } from './ui';

// Only compact, displayable fields cross the worker boundary, never release bodies/assets.
export interface IndexRow { id: string; files: boolean; title: string; searchable: string; added: string; released: string; update: boolean }
export interface LibraryIndex {
  software: string[]; files: string[]; softwareCount: number; filesCount: number;
  unfiltered?: { software: string[]; files: string[] };
}
export const emptyIndex: LibraryIndex = { software: [], files: [], softwareCount: 0, filesCount: 0 };
export type IndexSort = Exclude<LibrarySort, 'manual'>;
const collator = new Intl.Collator('zh-CN', { numeric: true, sensitivity: 'base' });
export function libraryRows(projects: Project[], notes: Map<string, RepositoryNote>): IndexRow[] {
  return projects.map(p => {
    const key = `${p.owner}/${p.repo}`;
    const note = notes.size ? notes.get(key.toLowerCase()) : undefined;
    return { id: p.id, files: p.managementMode === 'files', title: note?.name.trim() || note?.description.trim() || p.name,
      searchable: `${p.name} ${key} ${p.description} ${note?.name ?? ''} ${note?.description ?? ''}`,
      added: p.createdAt, released: p.release?.publishedAt ?? '', update: hasProjectUpdate(p) };
  });
}
export function prepareRows(rows: IndexRow[]) { return rows.map(r => ({ ...r, searchable: r.searchable.toLowerCase() })); }
export function sortRows(rows: IndexRow[], sort: IndexSort): IndexRow[] {
  const descending = (a: string, b: string) => a < b ? 1 : a > b ? -1 : 0;
  // Sort a copy: switching criteria must not change stable tie order on a later sort.
  return rows.slice().sort((a, b) => {
    if (sort === 'name' || sort === 'nameDesc') return collator.compare(a.title, b.title) * (sort === 'name' ? 1 : -1);
    if (sort === 'released') return descending(a.released, b.released) || descending(a.added, b.added);
    if (sort === 'updated' && a.update !== b.update) return Number(b.update) - Number(a.update);
    return descending(a.added, b.added);
  });
}
export function filterRows(rows: IndexRow[], query: string, includeUnfiltered = false): LibraryIndex {
  const out: LibraryIndex = { software: [], files: [], softwareCount: 0, filesCount: 0 };
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (includeUnfiltered && terms.length) out.unfiltered = { software: [], files: [] };
  for (const row of rows) {
    const kind = row.files ? 'files' : 'software';
    if (row.files) out.filesCount++; else out.softwareCount++;
    out.unfiltered?.[kind].push(row.id);
    if (terms.every(t => row.searchable.includes(t))) out[kind].push(row.id);
  }
  return out;
}
export type IndexRequest = { generation: number; rows?: IndexRow[]; sort: IndexSort; query: string; includeUnfiltered: boolean };
export type IndexReply = { generation: number; result: LibraryIndex };

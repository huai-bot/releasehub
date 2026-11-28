import { filterRows, prepareRows, sortRows, type IndexRequest, type IndexReply, type IndexRow } from './library-index';
let rows: IndexRow[] = [], sorted: IndexRow[] = [];
let previousSort = '';
self.onmessage = ({ data }: MessageEvent<IndexRequest>) => {
  if (data.rows) { rows = prepareRows(data.rows); previousSort = ''; }
  if (previousSort !== data.sort) { sorted = sortRows(rows, data.sort); previousSort = data.sort; }
  self.postMessage({ generation: data.generation, result: filterRows(sorted, data.query, data.includeUnfiltered) } satisfies IndexReply);
};

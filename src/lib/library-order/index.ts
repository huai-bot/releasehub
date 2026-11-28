export type DropEdge = 'before' | 'after';

/** Keep surviving manual ranks first; append new projects in the existing newest-first order. */
export function reconcileLibraryOrder(defaultOrder: string[], saved: readonly string[]): string[] {
  if (!saved.length) return defaultOrder;
  const remaining = new Set(defaultOrder);
  const order = saved.filter(id => remaining.delete(id));
  order.push(...remaining);
  return order;
}

/** Reorder only matching slots. Hidden projects stay at their original positions. */
export function moveLibraryOrder(order: readonly string[], matching: readonly string[], from: string, target: string, edge: DropEdge): { order: string[]; index: number } | null {
  if (from === target) return null;
  const start = matching.indexOf(from), end = matching.indexOf(target);
  if (start < 0 || end < 0) return null;
  const insert = end + (edge === 'after' ? 1 : 0) - Number(start < end);
  if (start === insert) return null;
  const moved = matching.slice();
  moved.splice(start, 1);
  moved.splice(insert, 0, from);
  if (order === matching) return { order: moved, index: insert };
  const matchingIds = new Set(matching);
  let index = 0;
  return { order: order.map(id => matchingIds.has(id) ? moved[index++] : id), index: insert };
}

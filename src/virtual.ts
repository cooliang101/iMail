export function virtualRange(total: number, scrollTop: number, viewportHeight: number, rowHeight: number, overscan: number) {
  if (total <= 0 || rowHeight <= 0 || viewportHeight < 0) return { start: 0, end: 0 };
  const start = Math.max(0, Math.floor(Math.max(0, scrollTop) / rowHeight) - Math.max(0, overscan));
  const end = Math.min(total, Math.ceil((Math.max(0, scrollTop) + viewportHeight) / rowHeight) + Math.max(0, overscan));
  return { start, end };
}

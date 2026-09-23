import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";

/** Nearest scrolling ancestor — Layout's <main> for page content. */
function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const oy = getComputedStyle(p).overflowY;
    if (oy === "auto" || oy === "scroll") return p;
  }
  return null;
}

/**
 * Top of `el` inside `scroller`'s scrolled content. Walks offsetTop instead of
 * getBoundingClientRect: the UI scale uses CSS `zoom` on <html>, and rects vs
 * scrollTop disagree about zoomed units between Chromium versions, while
 * offsetTop and scrollTop are always in the same layout pixels.
 */
function offsetWithin(el: HTMLElement, scroller: HTMLElement): number {
  let y = 0;
  let node: HTMLElement | null = el;
  while (node && node !== scroller) {
    y += node.offsetTop;
    node = node.offsetParent as HTMLElement | null;
  }
  return y;
}

/** Last index whose start offset is <= y (offsets has count + 1 entries). */
function indexAt(offsets: number[], y: number): number {
  let lo = 0;
  let hi = offsets.length - 2;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (offsets[mid] <= y) lo = mid;
    else hi = mid - 1;
  }
  return Math.max(0, lo);
}

/**
 * Fixed-height windowing against the page's own scroll container, so a 20k-file
 * list scrolls with the rest of the page (headers, filters) instead of in a box
 * inside a box. Heights are per item (folder headers differ from rows) but must
 * be known up front; that's what keeps this simple compared to measuring.
 *
 * Only re-renders when the visible index range changes, not on every scroll pixel.
 */
export function useVirtualList(heights: number[], overscanPx = 480) {
  const ref = useRef<HTMLDivElement>(null);
  const offsets = useMemo(() => {
    const o = new Array<number>(heights.length + 1);
    o[0] = 0;
    for (let i = 0; i < heights.length; i++) o[i + 1] = o[i] + heights[i];
    return o;
  }, [heights]);
  const offsetsRef = useRef(offsets);
  offsetsRef.current = offsets;

  const [range, setRange] = useState({ start: 0, end: Math.min(heights.length, 40) });

  const measure = useCallback(() => {
    const el = ref.current;
    const sc = el && scrollParent(el);
    const o = offsetsRef.current;
    const count = o.length - 1;
    if (!el || !sc || count === 0) {
      setRange((r) => (r.start === 0 && r.end === 0 ? r : { start: 0, end: 0 }));
      return;
    }
    const top = sc.scrollTop - offsetWithin(el, sc);
    const start = indexAt(o, top - overscanPx);
    const end = Math.min(count, indexAt(o, top + sc.clientHeight + overscanPx) + 1);
    setRange((r) => (r.start === start && r.end === end ? r : { start, end }));
  }, [overscanPx]);

  // Items changed (filter, collapse, density): recompute before paint.
  useLayoutEffect(measure, [offsets, measure]);

  useLayoutEffect(() => {
    const el = ref.current;
    const sc = el && scrollParent(el);
    if (!el || !sc) return;
    // Measured straight in the handler: it's a short offsetTop walk plus a binary
    // search, and setRange bails out unless the index range actually moved.
    const schedule = () => measure();
    sc.addEventListener("scroll", schedule, { passive: true });
    // The scroller resizing (window, zoom) or content above the list growing
    // (a banner, the duplicate finder) moves the window without a scroll event.
    const ro = new ResizeObserver(schedule);
    ro.observe(sc);
    for (const child of Array.from(sc.children)) ro.observe(child);
    return () => {
      sc.removeEventListener("scroll", schedule);
      ro.disconnect();
    };
  }, [measure]);

  return { ref, offsets, total: offsets[offsets.length - 1], start: range.start, end: range.end };
}

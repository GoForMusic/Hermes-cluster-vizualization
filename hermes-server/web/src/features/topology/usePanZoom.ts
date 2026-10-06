// Pan, zoom and fit for the map. The view is one value (x, y, scale); the wheel zooms around the pointer, dragging pans, a double click fits.
import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type RefObject } from 'react';
import { clamp } from '../../domain/format';
import type { Layout } from '../../domain/map/layout';
import { gridRef } from '../../domain/map/links';

export interface View { x: number; y: number; k: number }
interface Box { x: number; y: number; w: number; h: number }
export interface Inset { top: number; bottom: number }

interface Options {
  layout: Layout;
  focusId: string | null;
  interactive: boolean;
  inset?: Inset;
  /** How far fitting may enlarge the map: a TV shows one small cluster on a big screen and wants it to fill it. */
  maxFit?: number;
  onCursor?: (ref: string, zoom: number) => void;
}

export interface PanZoom {
  svgRef: RefObject<SVGSVGElement | null>;
  view: View;
  animate: boolean;
  panning: boolean;
  /** Was the pointer dragged (so that the click that ends a drag is not a selection)? */
  wasDragged: () => boolean;
  /** Keep the map where it is: the person is changing something on it (a box's size), so it must not be fitted again by itself. */
  hold: () => void;
  focus: (clusterId: string | null, animate?: boolean) => void;
  onPointerDown: (e: ReactPointerEvent<SVGSVGElement>) => void;
  onPointerMove: (e: ReactPointerEvent<SVGSVGElement>) => void;
}

const MARGIN = 28;
const DEFAULT_MAX_FIT = 1.7;

export function usePanZoom({ layout, focusId, interactive, inset, maxFit = DEFAULT_MAX_FIT, onCursor }: Options): PanZoom {
  const svgRef = useRef<SVGSVGElement>(null);
  const [view, setView] = useState<View>({ x: 0, y: 0, k: 1 });
  const [animate, setAnimate] = useState(false);
  const [panning, setPanning] = useState(false);
  const viewRef = useRef(view);
  viewRef.current = view;
  const moved = useRef(false); // the person moved the map: do not fit it again by itself
  const dragged = useRef(0);
  const top = inset?.top ?? 0, bottom = inset?.bottom ?? 0; // room for overlays (banner, HUD)

  const fitBox = useCallback((box: Box, anim: boolean) => {
    const svg = svgRef.current;
    if (!svg) return;
    const r = svg.getBoundingClientRect();
    if (!r.width || !r.height || !box.w || !box.h) return;
    const avail = r.height - top - bottom;
    const k = clamp(Math.min((r.width - MARGIN * 2) / box.w, (avail - MARGIN * 2) / box.h), 0.2, maxFit);
    setAnimate(anim);
    setView({ k, x: (r.width - box.w * k) / 2 - box.x * k, y: top + (avail - box.h * k) / 2 - box.y * k });
  }, [top, bottom, maxFit]);

  const focus = useCallback((clusterId: string | null, anim = false) => {
    moved.current = false;
    const c = clusterId ? layout.clusters.find((b) => b.id === clusterId) : undefined;
    fitBox(c ? { x: c.x, y: c.y, w: c.w, h: c.h } : { x: 0, y: 0, w: layout.w, h: layout.h }, anim);
  }, [layout, fitBox]);

  const fitToFocus = useCallback(() => focus(focusId && layout.clusters.some((c) => c.id === focusId) ? focusId : null, false), [focus, focusId, layout]);

  // fit again when the layout or the focus changes, and when the box is resized. A person who moved or zoomed the map keeps it as it is
  // when the layout changes (a box they stretched, a new pod); a different focus (the rotation moving to another cluster) fits again.
  const lastFocus = useRef(focusId);
  useEffect(() => {
    if (lastFocus.current !== focusId) { lastFocus.current = focusId; moved.current = false; }
    if (!moved.current) fitToFocus();
  }, [fitToFocus]);
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    const ro = new ResizeObserver(() => { if (!moved.current) fitToFocus(); });
    ro.observe(svg.parentElement ?? svg);
    return () => ro.disconnect();
  }, [fitToFocus]);

  useEffect(() => {
    const svg = svgRef.current;
    if (!interactive || !svg) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = svg.getBoundingClientRect();
      const px = e.clientX - r.left, py = e.clientY - r.top;
      const v = viewRef.current;
      const k = clamp(v.k * Math.exp(-e.deltaY * 0.0015), 0.2, Math.max(3, maxFit));
      moved.current = true;
      setAnimate(false);
      setView({ k, x: px - (px - v.x) * (k / v.k), y: py - (py - v.y) * (k / v.k) });
    };
    const onDouble = () => focus(null);
    svg.addEventListener('wheel', onWheel, { passive: false }); // it must be allowed to stop the page from scrolling
    svg.addEventListener('dblclick', onDouble);
    return () => { svg.removeEventListener('wheel', onWheel); svg.removeEventListener('dblclick', onDouble); };
  }, [interactive, focus, maxFit]);

  const onPointerDown = useCallback((e: ReactPointerEvent<SVGSVGElement>) => {
    if (!interactive) return;
    const start = { x: e.clientX, y: e.clientY, vx: viewRef.current.x, vy: viewRef.current.y };
    dragged.current = 0;
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - start.x, dy = ev.clientY - start.y;
      dragged.current = Math.max(dragged.current, Math.hypot(dx, dy));
      if (dragged.current >= 4) {
        moved.current = true;
        setPanning(true);
        setAnimate(false);
        setView((v) => ({ ...v, x: start.vx + dx, y: start.vy + dy }));
      }
    };
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      setPanning(false);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }, [interactive]);

  const onPointerMove = useCallback((e: ReactPointerEvent<SVGSVGElement>) => {
    if (!onCursor) return;
    const r = e.currentTarget.getBoundingClientRect();
    const v = viewRef.current;
    onCursor(gridRef((e.clientX - r.left - v.x) / v.k, (e.clientY - r.top - v.y) / v.k), v.k);
  }, [onCursor]);

  return { svgRef, view, animate, panning, wasDragged: () => dragged.current >= 4, hold: () => { moved.current = true; }, focus, onPointerDown, onPointerMove };
}

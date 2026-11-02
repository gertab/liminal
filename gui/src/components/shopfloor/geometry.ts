import type { MmDef, Vec2 } from "./types";

/**
 * Keeps a target point where the MM's centre can actually go (floor minus its radius), rounded to
 * the centimetre.
 */
export function clampToReach(point: Vec2, mm: MmDef, width: number, height: number): Vec2 {
  const r = mm.radius_m;
  const clamp = (value: number, max: number) => Math.min(Math.max(Math.round(value * 100) / 100, r), max - r);
  return { x: clamp(point.x, width), y: clamp(point.y, height) };
}

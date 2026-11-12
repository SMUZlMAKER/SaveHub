// Цвета тёмной темы приложения (frontend/src/app.css), чтобы ролик выглядел как SaveHub.
export const theme = {
  bg: "#16191d",
  surface: "#1f2329",
  text: "#e6e9ed",
  muted: "#9aa5b1",
  accent: "#6a9bf0",
  accentStrong: "#2f6fde",
  accentSoft: "#223252",
  ok: "#5cc98a",
  danger: "#ef6b6b",
  border: "#2e343c",
  font: '"Segoe UI", system-ui, sans-serif',
  mono: '"Cascadia Mono", Consolas, monospace',
} as const;

export const FPS = 30;
export const WIDTH = 1920;
export const HEIGHT = 1080;
/** Длительность перехода между сценами, кадров. */
export const TRANSITION = 15;

export const sec = (s: number) => Math.round(s * FPS);

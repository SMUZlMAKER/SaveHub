import type { CSSProperties, ReactNode } from "react";
import {
  AbsoluteFill,
  Img,
  interpolate,
  spring,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { theme } from "../theme";

export const Background = ({ children }: { children: ReactNode }) => (
  <AbsoluteFill
    style={{
      background: `radial-gradient(circle at 50% 35%, ${theme.accentSoft} 0%, ${theme.bg} 65%)`,
      color: theme.text,
      fontFamily: theme.font,
    }}
  >
    {children}
  </AbsoluteFill>
);

export const Logo = ({ size }: { size: number }) => (
  <Img src={staticFile("app-icon.svg")} style={{ width: size, height: size }} />
);

/** Плавное появление снизу с задержкой в кадрах. */
export const useAppear = (delay = 0) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const p = spring({ frame: frame - delay, fps, config: { damping: 200 } });
  return {
    opacity: p,
    transform: `translateY(${interpolate(p, [0, 1], [24, 0])}px)`,
  } satisfies CSSProperties;
};

export const Appear = ({
  delay = 0,
  style,
  children,
}: {
  delay?: number;
  style?: CSSProperties;
  children: ReactNode;
}) => <div style={{ ...useAppear(delay), ...style }}>{children}</div>;

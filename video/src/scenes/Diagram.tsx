import type { ReactNode } from "react";
import { AbsoluteFill, interpolate, useCurrentFrame } from "remotion";
import { sec, theme } from "../theme";
import { Appear, Background } from "./common";

const Box = ({ title, children }: { title: string; children: ReactNode }) => (
  <div
    style={{
      width: 460,
      minHeight: 300,
      background: theme.surface,
      border: `2px solid ${theme.border}`,
      borderRadius: 28,
      padding: 36,
      display: "flex",
      flexDirection: "column",
      gap: 20,
    }}
  >
    <div style={{ fontSize: 30, color: theme.muted }}>{title}</div>
    {children}
  </div>
);

const Chip = ({ children, color = theme.accent }: { children: ReactNode; color?: string }) => (
  <div
    style={{
      alignSelf: "flex-start",
      fontSize: 34,
      fontWeight: 600,
      padding: "8px 22px",
      borderRadius: 999,
      background: theme.accentSoft,
      color,
    }}
  >
    {children}
  </div>
);

/** Стрелка, которая «прорисовывается» слева направо. */
const Arrow = ({ label, start }: { label: string; start: number }) => {
  const frame = useCurrentFrame();
  const p = interpolate(frame, [start, start + sec(0.6)], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  return (
    <div style={{ width: 200, display: "flex", flexDirection: "column", alignItems: "center", gap: 12 }}>
      <div style={{ fontSize: 28, color: theme.accent, opacity: p }}>{label}</div>
      <svg width={200} height={40}>
        <line
          x1={0}
          y1={20}
          x2={180 * p}
          y2={20}
          stroke={theme.accent}
          strokeWidth={6}
          strokeLinecap="round"
        />
        <polygon points="176,6 200,20 176,34" fill={theme.accent} opacity={p} />
      </svg>
    </div>
  );
};

export const Diagram = () => (
  <Background>
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", gap: 110 }}>
      <Appear>
        <div style={{ fontSize: 72, fontWeight: 700 }}>Как это работает</div>
      </Appear>
      <div style={{ display: "flex", alignItems: "center" }}>
        <Appear delay={sec(0.4)}>
          <Box title="Папка игры">
            <div style={{ fontSize: 34, fontFamily: theme.mono }}>…\Game\saves</div>
            <div style={{ fontSize: 30, color: theme.muted }}>
              игра ничего не замечает
            </div>
          </Box>
        </Appear>
        <Arrow label="junction" start={sec(1.2)} />
        <Appear delay={sec(1.6)}>
          <Box title="Git-репозиторий">
            <div style={{ fontSize: 30, color: theme.muted }}>ветка на каждый профиль</div>
            <Chip>Аня</Chip>
            <Chip color={theme.ok}>Боря</Chip>
          </Box>
        </Appear>
        <Arrow label="push" start={sec(2.6)} />
        <Appear delay={sec(3)}>
          <Box title="GitHub">
            <div style={{ fontSize: 40, fontWeight: 600 }}>ваш репозиторий</div>
            <div style={{ fontSize: 30, color: theme.muted }}>
              забрать на другом ПК
            </div>
          </Box>
        </Appear>
      </div>
    </AbsoluteFill>
  </Background>
);

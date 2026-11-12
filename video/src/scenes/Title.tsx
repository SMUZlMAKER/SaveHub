import { AbsoluteFill, spring, useCurrentFrame, useVideoConfig } from "remotion";
import { sec, theme } from "../theme";
import { Appear, Background, Logo } from "./common";

export const Title = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const scale = spring({ frame, fps, config: { damping: 12, mass: 0.6 } });

  return (
    <Background>
      <AbsoluteFill
        style={{ justifyContent: "center", alignItems: "center", gap: 36 }}
      >
        <div style={{ transform: `scale(${scale})` }}>
          <Logo size={260} />
        </div>
        <Appear delay={sec(0.4)}>
          <div style={{ fontSize: 140, fontWeight: 800, letterSpacing: -2 }}>
            SaveHub
          </div>
        </Appear>
        <Appear delay={sec(0.9)}>
          <div style={{ fontSize: 48, color: theme.muted, textAlign: "center" }}>
            Свои сохранения у каждого игрока — на любом компьютере
          </div>
        </Appear>
      </AbsoluteFill>
    </Background>
  );
};

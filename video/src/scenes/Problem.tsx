import { AbsoluteFill } from "remotion";
import { sec, theme } from "../theme";
import { Appear, Background } from "./common";

const lines = ["Один компьютер.", "Одна игра.", "Одна папка сохранений."];

export const Problem = () => (
  <Background>
    <AbsoluteFill
      style={{ justifyContent: "center", paddingLeft: 220, gap: 18 }}
    >
      {lines.map((line, i) => (
        <Appear key={line} delay={sec(0.2 + i * 0.7)}>
          <div style={{ fontSize: 96, fontWeight: 700 }}>{line}</div>
        </Appear>
      ))}
      <Appear delay={sec(3)} style={{ marginTop: 40 }}>
        <div style={{ fontSize: 56, color: theme.danger }}>
          Кто-то сел за чужой сейв — и прогресс пропал.
        </div>
      </Appear>
    </AbsoluteFill>
  </Background>
);

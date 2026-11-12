import { AbsoluteFill } from "remotion";
import { team } from "../shots";
import { sec, theme } from "../theme";
import { Appear, Background, Logo } from "./common";

export const Outro = () => (
  <Background>
    <AbsoluteFill style={{ justifyContent: "center", alignItems: "center", gap: 30 }}>
      <Appear style={{ display: "flex", alignItems: "center", gap: 32 }}>
        <Logo size={150} />
        <div style={{ fontSize: 110, fontWeight: 800 }}>SaveHub</div>
      </Appear>
      <Appear delay={sec(0.5)}>
        <div style={{ fontSize: 44, color: theme.muted }}>
          Команда #174 · DevDays Осень 2026
        </div>
      </Appear>
      <Appear delay={sec(1)}>
        <div
          style={{
            fontSize: 34,
            color: theme.text,
            display: "flex",
            flexWrap: "wrap",
            justifyContent: "center",
            columnGap: 36,
            rowGap: 8,
            maxWidth: 1300,
          }}
        >
          {team.map((name) => (
            <span key={name} style={{ whiteSpace: "nowrap" }}>
              {name}
            </span>
          ))}
        </div>
      </Appear>
      <Appear delay={sec(1.6)} style={{ marginTop: 30 }}>
        <div style={{ fontSize: 48, fontFamily: theme.mono, color: theme.accent }}>
          github.com/SMUZlMAKER/SaveHub
        </div>
      </Appear>
    </AbsoluteFill>
  </Background>
);

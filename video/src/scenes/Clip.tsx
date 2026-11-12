import {
  AbsoluteFill,
  OffthreadVideo,
  interpolate,
  spring,
  staticFile,
  useCurrentFrame,
} from "remotion";
import type { Shot } from "../shots";
import { FPS, HEIGHT, WIDTH, sec, theme } from "../theme";
import { Appear, Background } from "./common";

// Запись экрана показываем уменьшенной в рамке, а подпись — под ней,
// чтобы подпись не закрывала кнопки внизу интерфейса.
const FRAME_TOP = 36;
const FRAME_HEIGHT = 860;
const FRAME_WIDTH = Math.round((FRAME_HEIGHT * 16) / 9);
const CAPTION_TOP = FRAME_TOP + FRAME_HEIGHT;

const Placeholder = ({ shot }: { shot: Shot }) => (
  <AbsoluteFill
    style={{
      justifyContent: "center",
      alignItems: "center",
      gap: 28,
      border: `6px dashed ${theme.border}`,
      borderRadius: 24,
    }}
  >
    <div style={{ fontSize: 36, color: theme.muted }}>Кадр {shot.id} · футажа пока нет</div>
    <div style={{ fontSize: 52, fontWeight: 600, maxWidth: 1300, textAlign: "center" }}>
      {shot.todo}
    </div>
    <div style={{ fontSize: 30, fontFamily: theme.mono, color: theme.accent }}>
      public/footage/{shot.file}
    </div>
  </AbsoluteFill>
);

const Caption = ({ text }: { text: string }) => (
  <div
    style={{
      position: "absolute",
      top: CAPTION_TOP,
      left: 0,
      width: WIDTH,
      height: HEIGHT - CAPTION_TOP,
      display: "flex",
      justifyContent: "center",
      alignItems: "center",
    }}
  >
    <Appear delay={sec(0.3)}>
      <div style={{ fontFamily: theme.font, fontSize: 48, fontWeight: 600, color: theme.text }}>
        {text}
      </div>
    </Appear>
  </div>
);

/** Плашка «Ctrl + Alt + F9» в правом верхнем углу кадра с записью. */
const Keys = ({ at, label }: { at: number; label: string[] }) => {
  const frame = useCurrentFrame();
  const start = sec(at);
  const end = start + sec(2.2);
  const opacity = interpolate(frame, [start, start + 6, end - 8, end], [0, 1, 1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  const pop = spring({ frame: frame - start, fps: FPS, config: { damping: 12 } });
  if (frame < start || frame > end) return null;
  return (
    <div
      style={{
        position: "absolute",
        top: FRAME_TOP + 32,
        right: (WIDTH - FRAME_WIDTH) / 2 + 32,
        display: "flex",
        alignItems: "center",
        gap: 14,
        opacity,
        transform: `scale(${0.8 + 0.2 * pop})`,
        transformOrigin: "top right",
      }}
    >
      {label.map((key, i) => (
        <div key={key} style={{ display: "flex", alignItems: "center", gap: 14 }}>
          {i > 0 && <span style={{ fontSize: 40, color: theme.muted }}>+</span>}
          <span
            style={{
              fontFamily: theme.font,
              fontSize: 44,
              fontWeight: 700,
              color: theme.text,
              background: theme.surface,
              border: `2px solid ${theme.accent}`,
              borderBottomWidth: 6,
              borderRadius: 14,
              padding: "8px 22px",
              boxShadow: "0 10px 30px rgba(0,0,0,0.5)",
            }}
          >
            {key}
          </span>
        </div>
      ))}
    </div>
  );
};

export const Clip = ({ shot }: { shot: Shot }) => {
  const rate = shot.playbackRate ?? 1;
  return (
    <Background>
      <div
        style={{
          position: "absolute",
          top: FRAME_TOP,
          left: (WIDTH - FRAME_WIDTH) / 2,
          width: FRAME_WIDTH,
          height: FRAME_HEIGHT,
          borderRadius: 24,
          overflow: "hidden",
          background: theme.bg,
          boxShadow: `0 0 0 2px ${theme.border}, 0 24px 60px rgba(0,0,0,0.5)`,
        }}
      >
        {shot.ready ? (
          <OffthreadVideo
            src={staticFile(`footage/${shot.file}`)}
            trimBefore={sec(shot.from)}
            trimAfter={sec(shot.from + shot.duration * rate)}
            playbackRate={rate}
            muted
            style={{
              width: "100%",
              height: "100%",
              objectFit: "cover",
              ...(shot.zoom && {
                transform: `scale(${shot.zoom.scale})`,
                transformOrigin: `${shot.zoom.x * 100}% ${shot.zoom.y * 100}%`,
              }),
            }}
          />
        ) : (
          <Placeholder shot={shot} />
        )}
      </div>
      {shot.keys && <Keys at={shot.keys.at} label={shot.keys.label} />}
      <Caption text={shot.caption} />
    </Background>
  );
};

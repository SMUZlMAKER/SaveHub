import { Fragment } from "react";
import { linearTiming, TransitionSeries } from "@remotion/transitions";
import { fade } from "@remotion/transitions/fade";
import { scenes, type Scene } from "./shots";
import { sec, TRANSITION } from "./theme";
import { Clip } from "./scenes/Clip";
import { Diagram } from "./scenes/Diagram";
import { Outro } from "./scenes/Outro";
import { Problem } from "./scenes/Problem";
import { Title } from "./scenes/Title";

export type VideoProps = {
  /** Во сколько раз ускорить записи экрана; заставки и схема идут как есть. */
  speed: number;
};

/** Сцены с учётом ускорения: кадр короче, футаж играет быстрее, плашки раньше. */
const scenesAt = (speed: number): Scene[] =>
  scenes.map((scene) =>
    scene.kind !== "clip" || speed === 1
      ? scene
      : {
          ...scene,
          duration: scene.duration / speed,
          playbackRate: (scene.playbackRate ?? 1) * speed,
          keys: scene.keys && { ...scene.keys, at: scene.keys.at / speed },
        },
  );

/** Длина ролика: сумма сцен минус наложения переходов. */
export const totalFrames = (speed = 1) =>
  scenesAt(speed).reduce((sum, s) => sum + sec(s.duration), 0) -
  TRANSITION * (scenes.length - 1);

const render = (scene: Scene) => {
  switch (scene.kind) {
    case "problem":
      return <Problem />;
    case "title":
      return <Title />;
    case "diagram":
      return <Diagram />;
    case "outro":
      return <Outro />;
    case "clip":
      return <Clip shot={scene} />;
  }
};

export const Video = ({ speed }: VideoProps) => (
  <TransitionSeries>
    {scenesAt(speed).map((scene, i) => (
      <Fragment key={i}>
        {i > 0 && (
          <TransitionSeries.Transition
            presentation={fade()}
            timing={linearTiming({ durationInFrames: TRANSITION })}
          />
        )}
        <TransitionSeries.Sequence durationInFrames={sec(scene.duration)}>
          {render(scene)}
        </TransitionSeries.Sequence>
      </Fragment>
    ))}
  </TransitionSeries>
);

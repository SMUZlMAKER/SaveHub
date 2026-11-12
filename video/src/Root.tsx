import { Composition } from "remotion";
import { FPS, HEIGHT, WIDTH } from "./theme";
import { totalFrames, Video } from "./Video";

/** Во сколько раз ускорены записи экрана (1 — исходная скорость). */
const SPEED = 1.3;

export const Root = () => (
  <Composition
    id="SaveHub"
    component={Video}
    defaultProps={{ speed: SPEED }}
    durationInFrames={totalFrames(SPEED)}
    fps={FPS}
    width={WIDTH}
    height={HEIGHT}
  />
);

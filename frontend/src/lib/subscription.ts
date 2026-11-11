export type Subscription = (() => void) & { ready: Promise<void> };

/** Register first, then read current state so events during registration cannot be lost. */
export function createSubscription<T>(
  register: (handler: (payload: T) => void) => Promise<() => void>,
  handler: (payload: T) => void,
  initialRead?: () => void,
  onError: (error: unknown) => void = console.error,
): Subscription {
  let disposed = false;
  let unlisten: (() => void) | undefined;
  const stop = (() => {
    disposed = true;
    unlisten?.();
    unlisten = undefined;
  }) as Subscription;
  stop.ready = register((payload) => {
    if (!disposed) handler(payload);
  }).then((cleanup) => {
    if (disposed) cleanup();
    else {
      unlisten = cleanup;
      initialRead?.();
    }
  }, (error) => {
    if (!disposed) {
      onError(error);
      // A registration failure must not leave the screen loading forever.
      initialRead?.();
    }
  });
  return stop;
}

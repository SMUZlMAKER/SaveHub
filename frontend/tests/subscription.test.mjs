import assert from 'node:assert/strict';
import test from 'node:test';
import { createSubscription } from '../src/lib/subscription.ts';

function delayedRegistration() {
  let handler;
  let complete;
  let cleaned = 0;
  return {
    register(callback) {
      handler = callback;
      return new Promise(resolve => { complete = resolve; });
    },
    ready() { complete(() => cleaned++); },
    emit(value) { handler(value); },
    get cleaned() { return cleaned; },
  };
}

test('initial read recovers a state change before listener registration completes', async () => {
  const registration = delayedRegistration();
  let backendKnown = false;
  let displayedKnown;
  let reads = 0;
  const read = () => { displayedKnown = backendKnown; reads++; };
  const stop = createSubscription(registration.register, read, read);
  assert.equal(reads, 0);
  // The first process poll completes before listen is registered: there is no event to deliver.
  backendKnown = true;
  registration.ready();
  await stop.ready;
  assert.equal(displayedKnown, true);
  assert.equal(reads, 1);
  backendKnown = false;
  registration.emit();
  assert.equal(displayedKnown, false);
  stop();
  assert.equal(registration.cleaned, 1);
});

test('unmount before registration cancels initial read and cleans up the late listener', async () => {
  const registration = delayedRegistration();
  let calls = 0;
  const callback = () => calls++;
  const stop = createSubscription(registration.register, callback, callback);
  stop();
  registration.emit();
  registration.ready();
  await stop.ready;
  stop();
  assert.equal(calls, 0);
  assert.equal(registration.cleaned, 1);
});

test('a registration failure reports the error and still loads the current state', async () => {
  const error = new Error('listen failed');
  let loaded = 0;
  const errors = [];
  const stop = createSubscription(
    () => Promise.reject(error), () => {}, () => loaded++, e => errors.push(e),
  );
  await stop.ready;
  assert.equal(loaded, 1);
  assert.deepEqual(errors, [error]);
  stop();
});

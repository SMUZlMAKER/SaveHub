import { invoke } from '@tauri-apps/api/core';
import type { DiagnosticEvent } from './types';

let clickTrace: string | undefined;
export function requestTrace(): string {
  return clickTrace ?? crypto.randomUUID();
}

/** Intentionally excludes command arguments, error messages and DOM text. */
export function recordDiagnostic(event: DiagnosticEvent): void {
  void invoke('record_diagnostic', { event }).catch(() => {
    // Diagnostics must never affect an action or recursively log its own failure.
  });
}

export function initializeDiagnostics(): void {
  document.addEventListener('click', (event) => {
    if (!(event.target instanceof Element)) return;
    const target = event.target.closest('button, a, [role="button"], input[type="checkbox"], select');
    if (!target) return;
    const traceId = crypto.randomUUID();
    clickTrace = traceId;
    const action = target.getAttribute('data-diagnostic') ?? `click.${target.tagName.toLowerCase()}`;
    recordDiagnostic({ traceId, action, phase: 'click' });
    setTimeout(() => { if (clickTrace === traceId) clickTrace = undefined; }, 0);
  }, true);
  window.addEventListener('error', () => {
    recordDiagnostic({ traceId: requestTrace(), action: 'window', phase: 'unhandledError' });
  });
  window.addEventListener('unhandledrejection', () => {
    recordDiagnostic({ traceId: requestTrace(), action: 'window', phase: 'unhandledRejection' });
  });
}

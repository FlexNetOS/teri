// Server-Sent Events helper for teri's live streams.
//
// teri's engine exposes real SSE endpoints the upstream MiroFish lacked:
//   GET /api/report/:id/agent-log/sse      — events: log, done, error
//   GET /api/report/:id/console-log/sse    — events: log, done, error
//   GET /api/report/:id/events             — events: progress, section, done, error
//   GET /api/simulation/:id/ticks/sse      — events: tick, done, error
//
// The base URL mirrors the axios instance (`api/index.js`): a relative path goes through the
// Vite dev proxy in dev and resolves same-origin in production; `VITE_API_BASE_URL` overrides
// both. EventSource is GET-only and cannot send custom headers — the SSE endpoints are
// locale-agnostic, so the absence of `Accept-Language` here is intentional and harmless.
const BASE = import.meta.env.VITE_API_BASE_URL || ''

/**
 * Open an SSE stream with named-event handlers and an automatic polling fallback.
 *
 * Distinguishing a *failed connection* (endpoint unavailable → fall back to polling) from a
 * *normal close* (server emitted `done`) is done via `readyState`: the browser auto-reconnects
 * on transient transport errors (readyState CONNECTING), so we only fall back when the socket is
 * CLOSED and we never completed. The server's own named `error` event (e.g. "reportNotFound")
 * also lands on the `error` listener; it is surfaced to `onServerError` before any fallback.
 *
 * @param {string} path - e.g. `/api/report/abc/agent-log/sse`
 * @param {Object} opts
 * @param {Object<string,function>} opts.events - map of event-name → (MessageEvent) => void
 * @param {function} [opts.onDone] - called when the server emits the terminal `done` event
 * @param {function} [opts.onServerError] - called with the `error` event's data string
 * @param {function} [opts.onFallback] - called once if the stream can't be established
 * @returns {function} close() - idempotent; stops the stream and suppresses fallback
 */
export function openSse(path, { events = {}, onDone, onServerError, onFallback } = {}) {
  let es
  let done = false
  let fellBack = false

  const fallback = () => {
    if (fellBack || done) return
    fellBack = true
    if (es) es.close()
    if (onFallback) onFallback()
  }

  if (typeof EventSource === 'undefined') {
    // Non-browser / unsupported environment — go straight to polling.
    fallback()
    return () => {}
  }

  try {
    es = new EventSource(`${BASE}${path}`)
  } catch (e) {
    console.warn('SSE open failed, falling back to polling:', e)
    fallback()
    return () => {}
  }

  for (const [name, fn] of Object.entries(events)) {
    es.addEventListener(name, fn)
  }

  es.addEventListener('done', () => {
    done = true
    es.close()
    if (onDone) onDone()
  })

  es.addEventListener('error', (e) => {
    // The server's named `error` event carries a data payload; a transport error does not.
    if (e && e.data && onServerError) {
      onServerError(e.data)
      return
    }
    // Transport-level error. CONNECTING means the browser is auto-retrying — leave it alone.
    // CLOSED before completion means the endpoint is unavailable — fall back to polling.
    if (!done && es.readyState === EventSource.CLOSED) {
      fallback()
    }
  })

  return () => {
    done = true
    if (es) es.close()
  }
}

# Phase 1: collab-web Group Chat UI - Research

**Researched:** 2026-03-30
**Domain:** Vanilla HTML/CSS/JS single-page application, REST API client, chat UI patterns
**Confidence:** HIGH — all core techniques are standard browser APIs; project already has pre-research documents in `.planning/research/`

---

## Summary

This phase builds a zero-dependency, zero-build-step group chat web UI that wraps the existing `collab-server` REST API in a familiar chat interface. The entire frontend lives in `collab-web/` as plain `.html`, `.css`, and `.js` files — no npm, no bundler, no framework. Opening `index.html` directly in a browser (or via a local HTTP server) is the full deployment story.

The server already exposes all required endpoints with permissive CORS (`CorsLayer::permissive()`), so cross-origin fetch from `file://` or any local HTTP server works without modification. The server also provides a `/history/{instance_id}` endpoint (returns messages where the user is sender OR recipient, ordered ASC) that is better suited for chat display than `/messages/{instance_id}` (recipient-only, ordered DESC). The planner should decide which endpoint to poll.

The `.planning/research/` directory contains two high-quality pre-research documents (`STACK.md` and `FEATURES.md`) that cover technology choices, CSS patterns, JavaScript polling patterns, and feature prioritization in detail. This research document synthesizes those findings and adds implementation specifics derived from reading the actual server code.

**Primary recommendation:** Use `/history/{instance_id}` for message display (shows full conversation context, ASC order suits chat), `GET /roster` for sidebar, `POST /messages` to send. All fetch calls are vanilla `fetch()`. No libraries required.

---

## Standard Stack

### Core

| Technology | Version | Purpose | Why Standard |
|------------|---------|---------|--------------|
| HTML5 | Living Standard | Page structure and semantics | Semantic elements clarify intent; works in every browser |
| CSS3 + Custom Properties | Living Standard | Layout, theming, components | CSS variables eliminate need for preprocessor; flexbox handles all layout |
| Vanilla JS (ES2020+) | ES2020 | API client, DOM, polling | `async/await`, optional chaining, `AbortController` all natively available in target browsers |

### Supporting (optional)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| marked.js | 15.x | Markdown rendering | Only if message content should render markdown; collab messages are plain text today — skip unless demo story requires it |

**No other libraries are needed.** No Lodash, no date library, no CSS framework. `Intl.RelativeTimeFormat` handles relative timestamps natively.

**Installation:** None. No `npm install`. No build step. Files are loaded directly via `<script src="...">` and `<link rel="stylesheet" href="...">` tags.

---

## Architecture Patterns

### Recommended Project Structure

```
collab-web/
  index.html           # Single entry point — all <script> and <link> tags
  css/
    reset.css          # Minimal reset (box-sizing, margin, padding zeroed)
    tokens.css         # :root custom properties only — colors, spacing, radii, fonts
    layout.css         # Body shell, header, sidebar, message feed, compose bar
    components.css     # Bubbles, roster items, buttons, inputs, badges
  js/
    sanitize.js        # esc() helper — HTML-escapes user content to prevent XSS
    api.js             # fetch wrappers: getHistory(), sendMessage(), getRoster(), updatePresence()
    render.js          # messageTemplate(), rosterTemplate(), renderMessages(), renderRoster()
    poll.js            # Recursive setTimeout polling loop + visibilitychange handler
    app.js             # Init: reads localStorage, prompts for identity, wires events, starts poll
```

**Script load order in index.html (no ES modules needed — files share global scope intentionally):**

```html
<link rel="stylesheet" href="css/reset.css">
<link rel="stylesheet" href="css/tokens.css">
<link rel="stylesheet" href="css/layout.css">
<link rel="stylesheet" href="css/components.css">

<script src="js/sanitize.js"></script>
<script src="js/api.js"></script>
<script src="js/render.js"></script>
<script src="js/poll.js"></script>
<script src="js/app.js"></script>
```

### Pattern 1: Three-Zone Flexbox Layout

```
┌─────────────────────────────────────────┐
│  header (instance ID, server URL, status│  fixed height
├──────────────────┬──────────────────────┤
│  message feed    │  roster sidebar      │  flex: 1, overflow-y: auto
│  (flex column)   │  (fixed width ~220px)│
├─────────────────────────────────────────┤
│  compose bar (recipient + input + send) │  fixed height
└─────────────────────────────────────────┘
```

```css
/* Source: .planning/research/STACK.md */
body {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
}
.message-feed {
  flex: 1;
  overflow-y: auto;
  min-width: 0;   /* CRITICAL: prevents flex item from refusing to shrink */
}
```

### Pattern 2: Recursive setTimeout Polling with AbortController

**What:** Next poll fires only after current fetch resolves. Prevents request stacking on slow servers.

```javascript
// Source: .planning/research/STACK.md
let pollController = null;

async function poll() {
  if (document.visibilityState === 'hidden') {
    schedulePoll();
    return;
  }
  pollController = new AbortController();
  try {
    const res = await fetch(`${serverUrl}/history/${instanceId}`, {
      signal: pollController.signal
    });
    if (res.ok) {
      const messages = await res.json();
      renderMessages(messages);
      setConnectionStatus('connected');
    } else {
      setConnectionStatus('error');
    }
  } catch (err) {
    if (err.name !== 'AbortError') {
      setConnectionStatus('error');
    }
  }
  schedulePoll();
}

function schedulePoll() {
  setTimeout(poll, 2000);
}

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible') poll();
});
```

### Pattern 3: Deduplication via Hash Set

**What:** Server returns full last-hour history on every poll. Set prevents re-rendering known messages.

```javascript
// Source: .planning/research/STACK.md
const rendered = new Set();

function renderMessages(messages) {
  // messages arrive ASC from /history endpoint
  for (const msg of messages) {
    if (rendered.has(msg.hash)) continue;
    rendered.add(msg.hash);
    messageList.insertAdjacentHTML('beforeend', buildBubble(msg));
  }
}
```

### Pattern 4: CSS Overflow Anchor for Auto-Scroll

**What:** Browser-native pinning to bottom. No `scrollTop = scrollHeight` needed.

```css
/* Source: .planning/research/STACK.md */
#messages > * { overflow-anchor: none; }
#scroll-anchor { overflow-anchor: auto; height: 1px; }
```

**Safari fallback** (overflow-anchor arrived in Safari 16.4, 2023): after appending a new bubble, call `scrollAnchor.scrollIntoView()` only when user was already near the bottom.

```javascript
function isNearBottom(el) {
  return el.scrollHeight - el.scrollTop - el.clientHeight < 80;
}
```

### Pattern 5: Message Grouping Logic

**What:** Consecutive messages from same sender within 60s collapse sender name/avatar.

```javascript
// Source: .planning/research/FEATURES.md — grouping pattern
function buildBubble(msg, prevMsg) {
  const isContinuation = prevMsg
    && prevMsg.sender === msg.sender
    && (new Date(msg.timestamp) - new Date(prevMsg.timestamp)) < 60000;

  return `
    <div class="message ${msg.sender === myInstance ? 'outgoing' : 'incoming'}
                        ${isContinuation ? 'continuation' : ''}">
      ${!isContinuation ? `<span class="sender-name">${esc(msg.sender)}</span>` : ''}
      <div class="bubble">${esc(msg.content)}</div>
      <span class="timestamp" title="${msg.timestamp}">${relativeTime(msg.timestamp)}</span>
    </div>`;
}
```

### Pattern 6: Content Sanitization

**What:** Escape all server-returned strings before inserting into HTML. No framework means no automatic escaping.

```javascript
// Source: .planning/research/STACK.md
function esc(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
```

### Pattern 7: Identity + Settings Persistence

```javascript
// Source: .planning/research/STACK.md
function getInstanceId() {
  let id = localStorage.getItem('collab_instance');
  if (!id) {
    id = prompt('Enter your instance ID (e.g. worker1):');
    if (id) localStorage.setItem('collab_instance', id.trim());
  }
  return id?.trim() || null;
}
```

Store server URL as `collab_server` (default `http://localhost:8000`). Provide an in-UI input that writes to localStorage and triggers reload.

### Pattern 8: Presence Update on Load

The server has a `PUT /presence/{instance_id}` endpoint. Call it on page load so the current user appears in the roster for other windows:

```javascript
// Source: server lib.rs — PresenceUpdate { role: Option<String> }
async function updatePresence(instanceId, role = '') {
  await fetch(`${serverUrl}/presence/${instanceId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ role })
  });
}
```

### Anti-Patterns to Avoid

- **setInterval for polling:** Stacks requests if server is slow. Always use recursive setTimeout.
- **Full innerHTML replacement on each poll:** Causes scroll position loss and flicker. Use insertAdjacentHTML with dedup Set.
- **Unescaped user content in template literals:** XSS vector. Always call `esc()` on server-returned strings.
- **scrollTop = scrollHeight unconditionally:** Interrupts user scrolling up to read history. Check proximity to bottom first.
- **Absolute ISO timestamps displayed raw:** Looks like a debug log. Use relative time ("just now", "2 min ago").

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Relative timestamps | Custom time diff logic | `Intl.RelativeTimeFormat` | Built into every target browser; handles locale, edge cases |
| Scroll pinning to bottom | Custom JS scrollHeight tracking | CSS `overflow-anchor` (+ 5-line JS fallback for Safari) | Browser-native; passive, doesn't fight user scroll |
| Content sanitization | DOMPurify or custom HTML parser | Simple `esc()` helper (4 string replacements) | Adequate for this use case — no HTML allowed in messages |
| Request deduplication | Timestamp-based comparison | Hash Set | Server provides stable SHA1 hash per message; O(1) lookup |

**Key insight:** For a demo tool of this size, hand-rolling anything more complex than a 4-line escape function is over-engineering. The browser already provides all required primitives.

---

## API Contract (Verified from Server Source)

All endpoints confirmed by reading `/Users/operator/code/claude-ipc/collab-server/src/lib.rs`.

| Endpoint | Method | Purpose | Notes |
|----------|--------|---------|-------|
| `/history/{instance_id}` | GET | Messages where user is sender OR recipient, last hour, ASC | Best for chat display |
| `/messages/{instance_id}` | GET | Messages where user is recipient only, last hour, DESC | Not ideal for chat — recipient-only, wrong order |
| `/messages` | POST | Send a message | Body: `{ sender, recipient, content, refs[] }` |
| `/roster` | GET | Online workers (presence table + senders), last hour | Returns `WorkerInfo[]` |
| `/presence/{instance_id}` | PUT | Register/update presence | Body: `{ role: string \| null }` |

**Message shape (from server):**
```json
{
  "id": "uuid",
  "hash": "sha1-40-hex",
  "sender": "instance_id",
  "recipient": "instance_id",
  "content": "string (max 4096 chars)",
  "refs": ["hash1", "hash2"],
  "timestamp": "2026-03-30T14:22:11Z"
}
```

**WorkerInfo shape (from server):**
```json
{
  "instance_id": "string",
  "role": "string (empty string when not set)",
  "last_seen": "2026-03-30T14:22:11Z",
  "message_count": 5
}
```

**Validation rules (from server source):**
- `instance_id`: max 64 chars, alphanumeric + `-` + `_` only
- `content`: max 4096 chars
- `refs`: max 20 items, each max 64 chars
- `role`: max 256 chars

**CORS:** Server uses `CorsLayer::permissive()` — all origins allowed, including `file://`. No special headers needed in fetch calls.

**Auth:** Server supports optional Bearer token (`COLLAB_TOKEN` env var). If set, all requests need `Authorization: Bearer <token>`. Default local setup has auth disabled. The UI should support an optional token field in settings.

---

## Common Pitfalls

### Pitfall 1: Polling the Wrong Endpoint

**What goes wrong:** Using `GET /messages/{instance_id}` instead of `GET /history/{instance_id}` for chat display. The messages endpoint returns only incoming messages (recipient = me), so the user can't see their own sent messages in the chat.

**Why it happens:** REQUIREMENTS.md references `/messages/{instance_id}` but the server has a `/history` endpoint that returns both sides of the conversation in chronological order — a much better fit for chat display.

**How to avoid:** Poll `/history/{instance_id}` for the message feed. The `/messages` endpoint is designed for the CLI's `collab list` command (inbox-style), not for group chat rendering.

**Warning signs:** Sent messages never appear in the chat until you receive a reply referencing them.

### Pitfall 2: setInterval Request Stacking

**What goes wrong:** setInterval fires a new fetch every 2s regardless of whether the previous completed. If the server takes 3s to respond, requests pile up and responses arrive out of order.

**Why it happens:** setInterval is the intuitive choice for "do this every N seconds."

**How to avoid:** Use recursive setTimeout — schedule the next poll only after `await fetch(...)` resolves.

**Warning signs:** Message list flickers, messages appear then disappear, console shows many in-flight requests.

### Pitfall 3: XSS via Unescaped Message Content

**What goes wrong:** `messageList.innerHTML += \`<div>${msg.content}</div>\`` allows script injection if any message contains `<script>` or event handler attributes.

**Why it happens:** Template literals are convenient but don't auto-escape.

**How to avoid:** Always pass user-supplied strings through `esc()` before interpolation into HTML strings.

**Warning signs:** Messages containing `<` or `>` render as broken HTML or execute unexpectedly.

### Pitfall 4: Auto-Scroll Interrupting Manual Scroll

**What goes wrong:** Unconditionally calling `scrollTop = scrollHeight` after each poll snaps the user back to the bottom while they are reading older messages.

**Why it happens:** "Always show newest" seems like the right default.

**How to avoid:** Check `isNearBottom()` before scrolling. The CSS `overflow-anchor` approach handles this passively — only scroll when the anchor element is within the viewport.

**Warning signs:** Users scrolling up to read history get snapped back on every 2s poll.

### Pitfall 5: Empty Role Field in Roster

**What goes wrong:** Displaying an empty role badge or rendering `role: ""` as visible UI noise.

**Why it happens:** The server sets `role = ''` by default in the presence table. Workers that have never called `PUT /presence` with a role will have an empty string in the roster response.

**How to avoid:** Guard all role display: `${worker.role ? \`<span class="badge">${esc(worker.role)}</span>\` : ''}`.

**Warning signs:** Roster sidebar shows "(no role)" or blank badges next to every entry.

### Pitfall 6: File:// vs HTTP:// Origin

**What goes wrong:** Some browser security policies restrict localStorage, or apply different CORS handling when opening via `file://`.

**Why it happens:** `file://` protocol has tighter sandboxing in some contexts.

**How to avoid:** The UI should work from both `file://` and a local HTTP server. The server uses `CorsLayer::permissive()` which allows `file://` origins. localStorage works under `file://` in all target browsers. If a user hits issues, `python3 -m http.server 3000` in `collab-web/` is the simple fallback.

---

## Design Tokens

Recommended CSS custom properties for a dark, demo-ready theme:

```css
/* Source: .planning/research/STACK.md */
:root {
  --color-bg: #1a1a2e;
  --color-surface: #16213e;
  --color-bubble-out: #0f3460;
  --color-bubble-in: #2d2d44;
  --color-accent: #e94560;
  --color-text: #eaeaea;
  --color-text-muted: #888;
  --color-online: #4caf50;
  --color-offline: #f44336;
  --radius-bubble: 18px;
  --radius-bubble-tail: 4px;
  --spacing-bubble-pad: 10px 14px;
  --font-body: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  --font-mono: "SF Mono", "Fira Code", monospace;
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `setInterval` for polling | Recursive `setTimeout` | ~2018, widely documented | Prevents request stacking; cleaner abort |
| `scrollTop = scrollHeight` | CSS `overflow-anchor` | Chrome 56 (2017), Safari 16.4 (2023) | Passive scroll-pinning, doesn't fight user scroll |
| Full `innerHTML` replacement on update | `insertAdjacentHTML` + dedup Set | Established pattern, ~2015+ | Preserves scroll, avoids DOM thrash |
| `new Date().getTime()` math for relative time | `Intl.RelativeTimeFormat` | ES2018, fully supported | Locale-aware, no custom logic |

---

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| collab-server binary | All API calls | Built (not running) | Compiled in `/target/release/collab-server` | Start with `./target/release/collab-server` |
| Browser (Chrome/Firefox/Safari) | Rendering | Assumed present | Current versions | — |
| Local HTTP server (optional) | Serving index.html | `python3 -m http.server` | On all dev machines | `file://` also works |

**Missing dependencies with no fallback:**
- collab-server must be running during testing/demo. It is compiled but not currently running.

**Missing dependencies with fallback:**
- No local HTTP server needed — `file://` origin works with the permissive CORS setup.

---

## Validation Architecture

> `workflow.nyquist_validation` is explicitly `false` in `.planning/config.json`. This section is skipped.

---

## Open Questions

1. **Which endpoint to poll: `/history` vs `/messages`?**
   - What we know: `/history/{id}` returns both sent and received messages in ASC order (better for chat). `/messages/{id}` returns only received, in DESC order.
   - What's unclear: REQUIREMENTS.md references `/messages/{instance_id}` but `/history` is better for the stated goal.
   - Recommendation: Use `/history/{instance_id}` for the message feed. Document the choice in the plan.

2. **Auth token support in UI?**
   - What we know: Server supports optional Bearer token. Default local setup has auth disabled.
   - What's unclear: Do any target demo environments have auth enabled?
   - Recommendation: Add an optional "API token" field in the settings area (stored in localStorage). Send `Authorization: Bearer <token>` header if set, omit otherwise. Zero friction when token is not configured.

3. **Presence update frequency?**
   - What we know: `PUT /presence/{instance_id}` registers the user in the roster. It is not called automatically anywhere.
   - What's unclear: Should presence be refreshed on every message poll, or just on load?
   - Recommendation: Call `updatePresence()` on load, and refresh every 30s (not every 2s — presence doesn't need to match message polling cadence).

---

## Sources

### Primary (HIGH confidence)

- `/Users/operator/code/claude-ipc/collab-server/src/lib.rs` — authoritative API contract, all endpoints, validation rules, CORS setup
- `/Users/operator/code/claude-ipc/collab-server/src/db.rs` — database schema, confirmed presence table structure
- `/Users/operator/code/claude-ipc/.planning/research/STACK.md` — pre-researched technology stack, CSS patterns, JS patterns (2026-03-30)
- `/Users/operator/code/claude-ipc/.planning/research/FEATURES.md` — pre-researched feature landscape and prioritization (2026-03-30)
- `/Users/operator/code/claude-ipc/.planning/REQUIREMENTS.md` — project requirements (2026-03-30)
- `/Users/operator/code/claude-ipc/.planning/PROJECT.md` — project context and key decisions

### Secondary (MEDIUM confidence)

- CSS-Tricks: overflow-anchor for scroll pinning
- MDN: AbortController, Page Visibility API, insertAdjacentHTML, Intl.RelativeTimeFormat
- STACK.md sources: Ahmad Shadeed bubble CSS article, CSS Variables guide 2025

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all vanilla browser APIs, no external dependencies to verify
- Architecture: HIGH — patterns verified against server source code and pre-research documents
- Pitfalls: HIGH — `/history` vs `/messages` pitfall discovered by directly reading server code; others from pre-research

**Research date:** 2026-03-30
**Valid until:** 2026-04-30 (stable browser APIs; server code would need re-read if API changes)

---
name: playwright-browser-automation
description: "Cross-browser web automation and browsing on macOS using Playwright MCP (supporting Chromium/Chrome, Apple Safari WebKit, and Firefox). Use for interactive web navigation, clicking video players, filling forms, taking screenshots, and fullscreen playback."
mcp-servers:
  - playwright
---

# Playwright Cross-Browser Automation Guide

This skill guides the agent in using Playwright MCP tools (`mcp__playwright__*`) for reliable, multi-browser automation, interactive media playback, and web browsing on macOS.

> [!TIP]
> **Dynamic Server Coupling:** Loading this skill automatically loads and connects the `playwright` MCP server. If browser binaries are ever reported missing by Playwright, the host environment installs them via `npx -y playwright@1.57.0 install chromium webkit firefox`.

## 1. Supported Browsers

Playwright supports three major engine families on macOS:

- **`chromium`** (Default): Google Chrome, Microsoft Edge, Brave, Arc.
- **`webkit`**: Native Apple Safari engine for macOS.
- **`firefox`**: Mozilla Firefox Gecko engine.

---

## 2. Choosing Between Visible GUI and Headless Mode

The agent must automatically distinguish the purpose of the task:

### A. Visible GUI Mode (`headless: false`)

Use visible GUI mode when:

- The user asks to **"open in Chrome/Safari"**, **"show me a movie/video"**, **"open website on screen"**, or **"watch"**.
- You need the user to see the browser window on macOS.
- You need full-screen media playback.

**Example `mcp__playwright__playwright_navigate` with visible GUI:**

```json
{
  "url": "https://www.youtube.com/watch?v=...",
  "browserType": "chromium",
  "headless": false,
  "width": 1920,
  "height": 1080
}
```

*For Native Desktop Apps:*
If the user wants their standard Mac Chrome or Safari profile with existing logins/subscriptions, use `bash`:

```bash
open -a "Google Chrome" "https://..."
# or for Safari:
open -a "Safari" "https://..."
```

### B. Headless Mode (`headless: true`)

Use headless mode when:

- Scraping data, reading articles, extracting text from complex Single Page Applications (SPAs).
- Filling forms or taking screenshots in the background without disturbing the user.
- Running automated diagnostic checks.

```json
{
  "url": "https://example.com",
  "browserType": "chromium",
  "headless": true
}
```

---

## 3. Core Playwright MCP Tools Reference

- **`mcp__playwright__playwright_navigate`**: Navigate to any URL (`url`, `browserType: "chromium" | "webkit" | "firefox"`, `headless: boolean`, `width`, `height`, `timeout`).
- **`mcp__playwright__playwright_click`**: Click any button, link, or video player element with automatic waiting (`selector: "button.play", "#movie_player"`).
- **`mcp__playwright__playwright_fill`**: Type text into search inputs or form fields (`selector: "input[name='q']", value: "Dune 2"`).
- **`mcp__playwright__playwright_screenshot`**: Capture a screenshot to verify page content (`name: "screen.png"`).
- **`mcp__playwright__playwright_evaluate`**: Run custom JavaScript in the browser (`script: "document.querySelector('video')?.play()"`).
- **`mcp__playwright__playwright_press_key`**: Send keyboard events (e.g. `key: "Enter"` or `key: "f"` for fullscreen).
- **`mcp__playwright__playwright_get_visible_text`**: Get visible text content from the current page.
- **`mcp__playwright__playwright_get_page_content`**: Get full rendered HTML content of the page.

---

## 4. End-to-End Workflow for Media & Video Playback Requests

When asked to search, find, and play a movie or video in full screen:

### Step 1: Search & Rating Comparison
1. Navigate to Google or streaming portal (`mcp__playwright__playwright_navigate`).
2. Search for the query (e.g. `фільми онлайн українською з високим рейтингом`).
3. If requested to check multiple pages (e.g. 3 pages):
   - Extract search results on page 1 via `mcp__playwright__playwright_get_visible_text`.
   - Explicitly click to page 2 (e.g. `a[aria-label='Page 2']` or link text `2`) and extract text.
   - Click to page 3 and extract text.
   - Compare ratings (IMDb, Kinobaza, or portal rating) and select the highest-rated movie.

### Step 2: Open Specific Movie Page (Crucial Anti-Pattern Prevention)
> [!CAUTION]
> **NEVER stop at the portal home page / catalog!**
> Navigating to `https://uakino.best/` or `https://megogo.net/` is only a directory. You MUST click on the specific movie card/poster or navigate directly to the movie URL (e.g. `https://uakino.best/filmy/...`).

1. Click on the selected movie card or link (`mcp__playwright__playwright_click`).
2. Confirm the browser is on the movie's dedicated player page.

### Step 3: Start Playback & Handle Overlays
1. Locate the video player container or iframe.
2. Click the **Play** button (`mcp__playwright__playwright_click` on `.play`, `button[aria-label='Play']`, or `#player`).
3. If an ad overlay or cookie banner appears, click the close/skip button (`.close-ad`, `button:has-text('Пропустити')`).

### Step 4: Toggle Fullscreen Mode
1. Click the Fullscreen button on the player controls, OR
2. Send key `f` via `mcp__playwright__playwright_press_key({"key": "f"})`, OR
3. Execute JS: `mcp__playwright__playwright_evaluate({"script": "document.querySelector('video')?.requestFullscreen()"})`.

### Step 5: Verify Active Video Stream & Ad-Free Playback
Before marking the task as completed in `TaskGraph`:
1. Check video playback state with JavaScript:
   ```json
   {
     "script": "const v = document.querySelector('video'); return v ? { paused: v.paused, time: v.currentTime, duration: v.duration } : { error: 'No video tag found' };"
   }
   ```
2. Capture a verification screenshot via `mcp__playwright__playwright_screenshot`.
3. Verify that the video is streaming (`currentTime > 0`, `paused: false`) without blocking ad banners.

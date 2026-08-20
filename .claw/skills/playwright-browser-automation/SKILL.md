---
name: playwright-browser-automation
description: "Cross-browser web automation and browsing on macOS using Playwright MCP (supporting Chromium/Chrome, Apple Safari WebKit, and Firefox). Use for interactive web navigation, clicking video players, filling forms, taking screenshots, and fullscreen playback."
mcp-servers:
  - playwright
---

# Playwright Cross-Browser Automation Guide

This skill guides the agent in using Playwright MCP tools (`mcp__playwright__*`) for reliable, multi-browser automation and interactive web browsing on macOS.

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
- **`mcp__playwright__playwright_get_page_content`**: Get full rendered HTML content of the page.

---

## 4. End-to-End TaskGraph Workflow for Media/Movie Requests

When the user asks to find and open a movie/video:

1. **Search Phase:** Find the direct movie/trailer URL using `WebSearch`.
2. **Launch Phase:** Launch Playwright with `headless: false` (and desired `browserType: "chromium"` or `"webkit"`) using `mcp__playwright__playwright_navigate`.
3. **Playback Phase:** Click the play button or video player using `mcp__playwright__playwright_click` or `mcp__playwright__playwright_press_key` (`key: "f"`).
4. **Verification Phase:** Confirm video is loaded and playing before marking tasks completed in `TaskGraph`.

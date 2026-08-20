---
name: puppeteer-browser-automation
description: "Control web browsers using Puppeteer MCP for interactive navigation, form filling, clicking, scraping, screenshots, and fullscreen media playback. Use when automating web tasks, interacting with JavaScript-rendered SPAs, clicking video players, or when asked to open a page in Chrome."
---

# Puppeteer Browser Automation Guide

This skill guides the agent in using Puppeteer MCP tools (`mcp__puppeteer__*`) for both headless background automation and visible GUI fullscreen interactions.

## 1. When to Use Visible GUI vs. Headless Mode

The agent MUST distinguish between background data tasks and visual user tasks:

### A. Visible GUI Mode (`headless: false`)

Use visible GUI mode when:

- The user asks to **"open in Chrome"**, **"show me a movie/video"**, **"open website on screen"**, or **"watch"**.
- You need the user to see the browser window on macOS.
- You need full-screen media playback.

**How to trigger Fullscreen GUI in `mcp__puppeteer__puppeteer_navigate`:**

```json
{
  "url": "https://www.youtube.com/watch?v=...",
  "launchOptions": {
    "headless": false,
    "args": [
      "--start-fullscreen",
      "--start-maximized"
    ]
  }
}
```

*Alternative for Desktop Apps:*
If the user wants their standard Mac Chrome profile with existing logins/subscriptions, use `bash`:

```bash
open -a "Google Chrome" "https://..."
```

### B. Headless Mode (`headless: true` or default)

Use headless mode when:

- Scraping data, reading articles, extracting text from complex Single Page Applications (SPAs).
- Filling forms or taking screenshots in the background without disturbing the user.
- Running automated diagnostic checks.

```json
{
  "url": "https://example.com",
  "launchOptions": {
    "headless": true
  }
}
```

---

## 2. Core Puppeteer MCP Tools Reference

- **`mcp__puppeteer__puppeteer_navigate`**: Navigate to any URL with optional `launchOptions`.
- **`mcp__puppeteer__puppeteer_click`**: Click any button, link, or video player element (`selector: "#movie_player", "button.play"`).
- **`mcp__puppeteer__puppeteer_fill`**: Fill in search bars or login inputs (`selector: "input[name='q']", value: "Dune 2"`).
- **`mcp__puppeteer__puppeteer_screenshot`**: Take a screenshot to verify what is displayed on the page.
- **`mcp__puppeteer__puppeteer_evaluate`**: Run custom JavaScript in the browser console (e.g. `document.querySelector('video').play()`).

---

## 3. End-to-End TaskGraph Workflow for Media/Movie Requests

When the user asks to find and open a movie/video:

1. **Search Phase:** Find the direct movie/trailer URL using `WebSearch`.
2. **Launch Phase:** Launch browser with `launchOptions: { headless: false, args: ["--start-fullscreen"] }` using `mcp__puppeteer__puppeteer_navigate`.
3. **Playback Phase:** Click the play button or video player using `mcp__puppeteer__puppeteer_click`.
4. **Verification Phase:** Confirm video is loaded and playing before marking tasks completed in `TaskGraph`.

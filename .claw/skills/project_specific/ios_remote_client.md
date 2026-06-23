# Claw Controller — iOS Remote Client (Full Build from Scratch)

## 0. Мета-інформація

- **Директорія проєкту:** `/Users/dev/Documents/GitHub/claw-code/ClawController/`
- **Xcode проєкт:** `ClawController.xcodeproj`
- **Bundle ID:** `com.clawcode.controller`
- **Мінімальна iOS:** 17.0
- **Мова:** Swift 6 (strict concurrency)
- **UI Framework:** SwiftUI (no UIKit, except wrappers for WKWebView/TextKit2)
- **Архітектура:** TCA (The Composable Architecture) — modular, testable, unidirectional data flow
- **Targets:** iPhone + iPad (Universal)

---

## 1. Загальний Опис Продукту

**Claw Controller** — це преміальний iOS-клієнт для екосистеми `claw-code`. Додаток підключається до бекенду `claw-analog` через WebSocket (NDJSON), надаючи повний контроль над AI-агентом з телефону: чат, моніторинг задач, інспекція інструментів, логи, аналітика, налаштування — все в реальному часі.

Це **НЕ** MVP. Це повнофункціональний, production-ready додаток рівня топових IDE-клієнтів (Cursor Mobile, GitHub Copilot Chat) з преміальним дизайном та потужною архітектурою.

---

## 2. Архітектура (TCA + Clean Layers)

### 2.1 Шари Архітектури

```
┌─────────────────────────────────────────────────────┐
│                   Presentation Layer                │
│  SwiftUI Views + ViewModifiers + Design System      │
├─────────────────────────────────────────────────────┤
│                   Feature Layer (TCA)               │
│  Reducers + State + Actions + Effects per screen    │
├─────────────────────────────────────────────────────┤
│                   Domain Layer                      │
│  Models + UseCases + Protocols                      │
├─────────────────────────────────────────────────────┤
│                   Data / Infrastructure Layer       │
│  WebSocket Client + NDJSON Parser + Keychain +      │
│  CoreData/SwiftData + FileManager + Haptics         │
└─────────────────────────────────────────────────────┘
```

### 2.2 Модулі (Swift Packages / Targets)

Кожен модуль — окремий Swift Package або target для забезпечення чистих залежностей:

| Модуль | Опис |
|--------|------|
| `ClawCore` | Доменні моделі, протоколи, enums, DTO |
| `ClawNetworking` | WebSocket Manager, NDJSON StreamParser, HTTP Client, Connection State Machine |
| `ClawFeatures` | TCA Reducers для кожного екрану (Chat, Tasks, Tools, Logs, Settings, Analytics) |
| `ClawDesignSystem` | Токени дизайну, кольори, типографіка, компоненти UI, анімації, haptics |
| `ClawPersistence` | SwiftData моделі, Keychain wrapper, UserDefaults, session cache |
| `ClawApp` | Точка входу, AppReducer, навігація, deep links |

### 2.3 Залежності (SPM)

| Пакет | Версія | Призначення |
|-------|--------|-------------|
| `swift-composable-architecture` | latest | TCA — ядро архітектури |
| `swift-dependencies` | latest | DI для TCA |
| `swift-syntax-highlight` або кастомний | — | Підсвітка коду у чаті |
| `swift-markdown-ui` | latest | Рендеринг Markdown у повідомленнях |
| `KeychainAccess` | latest | Безпечне зберігання credentials |
| `SwiftLintPlugins` | latest | Лінтинг під час збірки |

---

## 3. Мережевий Рівень (Networking) — Глибока Реалізація

### 3.1 WebSocket Connection State Machine

```swift
enum ConnectionState: Equatable {
    case disconnected(reason: DisconnectReason)
    case connecting(attempt: Int)
    case authenticating
    case connected(since: Date)
    case reconnecting(attempt: Int, maxAttempts: Int)
    case error(ConnectionError)
}
```

**Вимоги:**
- `URLSessionWebSocketTask` з TLS pinning (опціонально)
- Exponential backoff reconnect (base 1s, max 30s, jitter ±20%)
- Heartbeat ping кожні 15 секунд з timeout 5s
- Auto-reconnect на `NWPathMonitor` network change events
- Background URLSession для підтримки з'єднання в background mode
- Connection quality indicator (latency measurement per ping)

### 3.2 NDJSON Stream Parser

```swift
actor NDJSONStreamParser {
    func parse(_ data: Data) -> AsyncThrowingStream<ServerEvent, Error>
}
```

Повинен парсити всі `type` з `claw-analog --output-format json`:
- `run_start` → `ServerEvent.runStarted(RunStartPayload)`
- `turn_start` → `ServerEvent.turnStarted(turn: Int)`
- `assistant_text_delta` → `ServerEvent.textDelta(String)`
- `assistant_turn` → `ServerEvent.turnCompleted(AssistantTurn)`
- `tool_result` → `ServerEvent.toolResult(ToolResult)`
- `run_end` → `ServerEvent.runEnded(ok: Bool)`
- `error` → `ServerEvent.error(String)`

### 3.3 Двонаправлена Комунікація

Клієнт повинен відправляти:
- `{ "type": "prompt", "text": "..." }` — новий промпт
- `{ "type": "cancel" }` — скасувати поточний run
- `{ "type": "ping" }` — heartbeat
- `{ "type": "config", "payload": {...} }` — динамічне оновлення конфігурації

### 3.4 Моделі Даних (Domain Models)

```swift
// Повідомлення чату
struct ChatMessage: Identifiable, Codable, Equatable {
    let id: UUID
    let role: MessageRole // .user, .assistant, .system, .toolResult
    let content: MessageContent // .text, .markdown, .code, .image, .toolCall, .error
    let timestamp: Date
    let metadata: MessageMetadata? // tokens, latency, model
    var status: MessageStatus // .sending, .sent, .streaming, .completed, .failed
}

// Нода задачі (Task Graph)
struct TaskNode: Identifiable, Codable, Equatable {
    let id: UUID
    let title: String
    let status: TaskStatus // .pending, .inProgress, .completed, .failed, .skipped
    let depth: Int
    var children: [TaskNode]
    let createdAt: Date
    var completedAt: Date?
    var duration: TimeInterval?
    var associatedToolCalls: [ToolCallReference]
}

// Виклик інструменту
struct ToolCall: Identifiable, Codable, Equatable {
    let id: String // tool_use_id
    let name: ToolName // enum: readFile, writeFile, listDir, grepWorkspace, glob...
    let arguments: [String: AnyCodable]
    let result: ToolResult?
    let startedAt: Date
    var completedAt: Date?
    var duration: TimeInterval?
    var isError: Bool
    var outputPreview: String? // truncated preview
    var outputFullLength: Int?
}

// Сесія
struct AgentSession: Identifiable, Codable, Equatable {
    let id: UUID
    let workspace: String
    let model: String
    let preset: String?
    let permission: PermissionLevel
    let startedAt: Date
    var endedAt: Date?
    var totalTurns: Int
    var totalToolCalls: Int
    var tokenUsage: TokenUsage
    var messages: [ChatMessage]
    var tasks: [TaskNode]
    var isRagEnabled: Bool
}
```

---

## 4. Екрани та UI/UX — Premium Design System

### 4.0 Design System Foundation

#### Палітра Кольорів (Dark-first)
```
Background:        #0A0A0F (майже чорний з синім відтінком)
Surface:           #12121A
Surface Elevated:  #1A1A28
Card:              #1E1E2E (з glass overlay)
Primary:           #7C5CFC (фіолетовий/індіго)
Primary Gradient:  #7C5CFC → #4F8EFF (indigo → electric blue)
Secondary:         #00D4AA (мінтовий/бірюзовий)
Accent:            #FF6B9D (рожевий акцент для alerts)
Success:           #00E676
Warning:           #FFB74D
Error:             #FF5252
Text Primary:      #EAEAFF
Text Secondary:    #8888AA
Text Tertiary:     #55556A
Code Background:   #0D1117 (GitHub Dark style)
```

#### Типографіка
- **Display:** SF Pro Rounded Bold (заголовки великих секцій)
- **Heading:** SF Pro Display Semibold
- **Body:** SF Pro Text Regular
- **Code:** SF Mono / JetBrains Mono (через Custom Font)
- **Caption:** SF Pro Text Light

#### Glass & Blur Effects
- `.ultraThinMaterial` для карток
- Custom `glassmorphism` modifier з gradient border
- Frosted glass navigation bar
- Blur transitions між екранами

#### Анімації
- Spring animations для всіх переходів (response: 0.5, dampingFraction: 0.75)
- Shimmer loading effect для streaming тексту
- Pulse animation для active connection indicator
- Particle effect при успішному завершенні задачі
- Typewriter effect для assistant text deltas
- Stagger animation для списків (кожен елемент з'являється з затримкою 0.05s)
- Smooth morphing transitions при зміні стану TaskNode
- Haptic feedback: `.light` на tap, `.medium` на action, `.success`/`.error` на результат

---

### 4.1 Tab Bar (Custom, не стандартний)

Кастомний floating tab bar знизу екрану з glassmorphism:

| Вкладка | Іконка | Опис |
|---------|--------|------|
| **Chat** | `bubble.left.and.bubble.right.fill` | Основний чат з агентом |
| **Tasks** | `chart.bar.doc.horizontal` | Task Graph / Plan Dashboard |
| **Tools** | `wrench.and.screwdriver.fill` | Інспекція Tool Calls |
| **Logs** | `terminal.fill` | Terminal-style log viewer |
| **More** | `ellipsis.circle.fill` | Analytics, Settings, Sessions |

Анімація: іконка active tab збільшується з bounce, під нею з'являється glowing dot з gradient.

---

### 4.2 Chat View (Головний Екран)

**Це серце додатку — має бути ІДЕАЛЬНИМ.**

#### Компоненти:
1. **Connection Status Bar** (top) — animated pill показує стан з'єднання:
   - 🟢 Connected (pulse animation, gradient glow)
   - 🟡 Reconnecting... (spinning animation)
   - 🔴 Disconnected (static, tap to reconnect)
   - Показує: model name, workspace path, latency

2. **Message List** (scrollable, lazy):
   - **User Bubble:** справа, gradient background (Primary), rounded corners 20px, shadow
   - **Assistant Bubble:** зліва, glass surface, subtle border
   - **Markdown rendering:** повна підтримка (headers, bold, italic, lists, links, tables)
   - **Code blocks:** syntax highlighting з мовою, copy button, line numbers, horizontal scroll
   - **Tool Call inline cards:** згортувані картки всередині assistant message:
     - Іконка інструменту + назва
     - Розгортається: показує arguments, result, duration
     - Колір border: green (success), red (error)
   - **Streaming indicator:** typing dots animation + partial text з cursor blink
   - **Message metadata:** tap на повідомлення → bottom sheet з: tokens used, latency, timestamp, model

3. **Smart Input Bar** (bottom):
   - Multi-line `TextEditor` з auto-resize (max 6 lines, then scroll)
   - Glass background
   - Ліва кнопка: attach context (file picker → send file path)
   - Права кнопка: send (gradient, animated on press)
   - Кнопка мікрофону для voice-to-text (Speech framework)
   - Quick actions row (swipe up): preset buttons ("Explain", "Fix", "Refactor", "Test")
   - При streaming: send button замінюється на stop button (червоний, pulsing)

4. **Scroll-to-bottom FAB:** floating action button з badge count нових повідомлень

#### Додаткові фішки Chat:
- **Swipe right** на повідомлення → копіювати
- **Swipe left** на повідомлення → retry / delete
- **Long press** → context menu: Copy, Share, Retry, View Raw JSON
- **Shake to clear** conversation (з confirmation dialog)
- **Pull to refresh** → reconnect WebSocket
- **Search** (magnifying glass у nav bar) → full-text search по історії чату

---

### 4.3 Task/Plan Dashboard

Візуалізація виконання `task.md` у реальному часі.

#### Два режими відображення (toggle):

**A) Tree View (default):**
- Ієрархічний список з indentation
- Кожен TaskNode має:
  - Status icon: ⬜ pending, 🔄 in_progress (spinning), ✅ completed, ❌ failed, ⏭️ skipped
  - Title text (bold якщо active)
  - Duration badge (якщо completed)
  - Child count badge
  - Expand/collapse з smooth animation
- Drag to reorder (future)

**B) Graph/Flow View:**
- Горизонтальний або вертикальний flow diagram
- Ноди з'єднані лініями (animated dash pattern для active)
- Zoom & pan з `MagnifyGesture` + `DragGesture`
- Ноди кольорово-кодовані за статусом
- Tap на ноду → detail sheet з associated tool calls

#### Dashboard Header Stats:
- Total tasks / Completed / Failed / In Progress — circular progress rings
- Estimated time remaining (based on average duration)
- Current phase indicator

---

### 4.4 Tools Inspector

Детальний інспектор всіх tool calls під час сесії.

#### Features:
1. **Timeline View:** хронологічний список всіх tool calls з часовою шкалою
   - Кожен виклик — картка з:
     - Tool icon (unique per tool type)
     - Tool name + arguments preview
     - Duration bar (visual, proportional)
     - Status badge (success/error)
     - Tap → expand: full arguments JSON, full output (scrollable), raw request/response

2. **Filter Bar (horizontal scroll chips):**
   - By tool name: `read_file`, `write_file`, `list_dir`, `grep_workspace`, `glob_workspace`, `git_diff`, `git_log`, `retrieve_context`, `ingest_context`
   - By status: All, Success, Error
   - By duration: Fast (<100ms), Normal, Slow (>2s)
   - Search by argument value (e.g., file path)

3. **Statistics Cards:**
   - Most used tool (pie chart)
   - Average duration per tool (bar chart)
   - Error rate per tool
   - Files most accessed

4. **Error Deep Dive:**
   - Filtered view showing only failed tool calls
   - Error message highlighted
   - Stack trace rendering (if available)

---

### 4.5 Terminal/Logs View

Повноцінний емулятор терміналу для перегляду логів.

#### Features:
1. **Terminal Aesthetic:**
   - Чорний фон (`#0D1117`), зелений текст для stdout, червоний для stderr
   - Моноширинний шрифт (SF Mono, 12pt)
   - Line numbers (toggleable)
   - Automatic scroll to bottom (з toggle button)
   - ANSI color code parsing та рендеринг

2. **Log Level Filtering:**
   - Chips: ALL, ERROR, WARN, INFO, DEBUG, TRACE
   - Кольорове маркування: 🔴 Error, 🟡 Warning, 🔵 Info, ⚪ Debug, 🔘 Trace

3. **Search & Highlight:**
   - Real-time search з highlighting matches
   - Regex support (toggle)
   - Match navigation (prev/next)

4. **Log Source Tabs:**
   - WebSocket raw stream
   - Parsed assistant responses
   - Tool execution logs
   - Connection events

5. **Export:** Share sheet для export логів як .txt або .json

---

### 4.6 Analytics Dashboard

Аналітика використання агента.

#### Widgets:
1. **Session Summary Card:**
   - Total sessions today/week/month
   - Total tokens used (input/output breakdown)
   - Total tool calls
   - Average session duration

2. **Token Usage Chart:**
   - Line chart: tokens over time (last 7 days)
   - Pie chart: input vs output tokens ratio
   - Cost estimation (based on model pricing)

3. **Tool Usage Heatmap:**
   - Grid showing tool frequency by hour of day
   - Most productive hours

4. **Model Performance:**
   - Average response time
   - Success rate
   - Turns per task

---

### 4.7 Settings Screen

#### Sections:

1. **Connection:**
   - Server URL (text field + validation indicator)
   - Port (default 8080)
   - WebSocket path (default `/ws`)
   - TLS toggle
   - Auto-reconnect toggle + max attempts
   - Heartbeat interval slider

2. **Agent Configuration:**
   - Model selector (dropdown: sonnet, opus, haiku, gpt-4o, custom)
   - Permission level (segmented: read-only, workspace-write, danger-full-access)
   - Preset selector (none, audit, explain, implement)
   - Max turns slider (1-100)
   - Language toggle (en/uk/ru)
   - RAG toggle + RAG URL

3. **Appearance:**
   - Theme (Dark/Light/System)
   - Font size slider
   - Code font selector
   - Haptic feedback toggle
   - Animation speed (normal/reduced)
   - Chat bubble style (modern/classic/minimal)

4. **Notifications:**
   - Push notifications for run completion
   - Error alerts
   - Connection lost alert
   - Sound toggle

5. **Data & Privacy:**
   - Clear chat history
   - Clear cached sessions
   - Export all data
   - Keychain management

6. **About:**
   - Version / Build
   - claw-code repo link
   - Licenses

---

### 4.8 Session Manager

#### Features:
- List of all saved sessions з preview
- Session card: workspace, model, date, turns count, status badge
- Tap → view full chat history (read-only replay mode)
- Swipe to delete
- Import/export session JSON
- Resume session (send session to claw-analog --session)

---

### 4.9 Onboarding Flow (перший запуск)

3-4 animated screens:
1. **Welcome** — logo animation, app name, tagline "Control your AI agent from anywhere"
2. **Connect** — ввести server URL, test connection button
3. **Configure** — обрати model, permission preset
4. **Ready** — success animation, "Start Chatting" CTA

---

## 5. Системні Інтеграції

### 5.1 Widgets (WidgetKit)
- **Small:** Connection status + last message preview
- **Medium:** Active task progress + token count
- **Large:** Mini chat view з останніми 3 повідомленнями

### 5.2 Live Activities (Dynamic Island)
- При активному run: показувати progress, current tool, turn count
- Compact: tool icon + turn number
- Expanded: progress bar + task name + cancel button

### 5.3 Shortcuts (App Intents)
- "Send prompt to Claw" — Siri Shortcut
- "Check agent status" — returns connection state
- "Start new session" — connect + send prompt

### 5.4 Haptics & Sound
- `UIImpactFeedbackGenerator` для тактильного фідбеку
- Custom sound при отриманні повідомлення (subtle notification)
- Error sound для failed tasks

### 5.5 Spotlight Search
- Indexing chat messages для пошуку через Spotlight

### 5.6 Handoff & Universal Links
- Передача активного чату між iPhone та iPad

---

## 6. Безпека та Якість Коду

### 6.1 Security
- Всі credentials (server URL, tokens) в Keychain
- Certificate pinning (опціонально, через Info.plist ATS config)
- No plaintext logging of sensitive data
- BiometricLock (FaceID/TouchID) для доступу до додатку (опціонально)

### 6.2 Code Quality
- **SwiftLint** інтегрований як Build Phase plugin
- **Strict concurrency** (Swift 6 mode)
- **Sendable** compliance для всіх моделей
- 100% TCA testability (кожен Reducer має unit tests)
- Preview providers для кожного SwiftUI View

### 6.3 Performance
- `LazyVStack` для всіх довгих списків
- `@Observable` макрос (iOS 17+) для state
- Image caching (if any images)
- Background processing для JSON parsing
- Memory warnings handling

---

## 7. TASK ROADMAP — Повний План Виконання

**УВАГА: Завдання розбиті на фази. Кожна фаза виконується послідовно. Всередині фази завдання виконуються в зазначеному порядку.**

### Phase 1: Project Foundation & Architecture (Фундамент)
1. Створити чистий Xcode проєкт з нуля у `/Users/dev/Documents/GitHub/claw-code/ClawController/`
   - Очистити існуючий код якщо потрібно, створити правильну структуру
   - Налаштувати targets, Bundle ID, Deployment Target (iOS 17.0)
   - Налаштувати Universal (iPhone + iPad)
2. Додати SPM залежності:
   - `swift-composable-architecture`
   - `MarkdownUI`
   - `KeychainAccess`
3. Створити модульну структуру папок:
   ```
   ClawController/
   ├── App/
   │   ├── ClawControllerApp.swift
   │   ├── AppReducer.swift
   │   └── AppDelegate.swift
   ├── Core/
   │   ├── Models/
   │   ├── Protocols/
   │   ├── Extensions/
   │   └── Utilities/
   ├── Networking/
   │   ├── WebSocketManager.swift
   │   ├── NDJSONParser.swift
   │   ├── ConnectionStateMachine.swift
   │   └── ServerEventTypes.swift
   ├── Features/
   │   ├── Chat/
   │   ├── Tasks/
   │   ├── Tools/
   │   ├── Logs/
   │   ├── Analytics/
   │   ├── Settings/
   │   ├── Sessions/
   │   └── Onboarding/
   ├── DesignSystem/
   │   ├── Tokens/
   │   ├── Components/
   │   ├── Modifiers/
   │   └── Animations/
   ├── Persistence/
   │   ├── SwiftDataModels/
   │   ├── KeychainService.swift
   │   └── SessionStorage.swift
   ├── SystemIntegrations/
   │   ├── Widgets/
   │   ├── LiveActivities/
   │   └── AppIntents/
   └── Resources/
       ├── Assets.xcassets
       ├── Fonts/
       └── Sounds/
   ```
4. Налаштувати SwiftLint (`.swiftlint.yml`) та додати Build Phase

### Phase 2: Design System & Core Components
5. Реалізувати Design System Foundation:
   - `ColorTokens.swift` — всі кольори з палітри
   - `TypographyTokens.swift` — шрифти та розміри
   - `SpacingTokens.swift` — відступи та розміри
   - `AnimationTokens.swift` — spring configurations
6. Створити базові UI компоненти:
   - `GlassmorphicCard` — картка з blur та gradient border
   - `GradientButton` — кнопка з градієнтом та haptic
   - `StatusBadge` — badge зі статусом (colored pill)
   - `ShimmerView` — loading shimmer effect
   - `PulsingDot` — animated connection indicator
   - `FloatingTabBar` — кастомний tab bar з glassmorphism
7. Створити ViewModifiers:
   - `.glassmorphic()` — glass effect
   - `.cardStyle()` — standard card styling
   - `.hapticFeedback(_:)` — trigger haptic
   - `.staggerAnimation(index:)` — stagger entry animation
   - `.shimmer()` — shimmer loading
8. Створити Animations:
   - `TypewriterEffect` — посимвольний набір тексту
   - `ParticleEffect` — particles при завершенні задачі
   - `MorphingShape` — трансформація shapes

### Phase 3: Networking Layer
9. Реалізувати `ConnectionStateMachine`:
   - Всі стани та переходи (disconnected → connecting → authenticating → connected)
   - Network path monitoring (`NWPathMonitor`)
   - Exponential backoff reconnect logic
10. Реалізувати `WebSocketManager`:
    - `URLSessionWebSocketTask` wrapper
    - Async/await API (`connect()`, `disconnect()`, `send(_:)`)
    - Ping/pong heartbeat (15s interval)
    - `AsyncStream<ServerEvent>` для отримання подій
    - Auto-reconnect з configurable policy
    - Connection quality measurement (latency)
11. Реалізувати `NDJSONStreamParser`:
    - Парсинг всіх типів `claw-analog` NDJSON events
    - Robust error handling (malformed JSON)
    - Backpressure handling
12. Реалізувати моделі даних:
    - `ChatMessage`, `TaskNode`, `ToolCall`, `AgentSession`
    - `ServerEvent` enum з associated values
    - `ClientCommand` enum для відправки
    - Codable conformance з custom coding keys

### Phase 4: Chat Feature (Core)
13. Реалізувати `ChatReducer` (TCA):
    - State: messages, connectionState, inputText, isStreaming, searchQuery
    - Actions: sendMessage, receiveEvent, toggleSearch, scrollToBottom, retry, cancel
    - Effects: WebSocket subscription, message sending, haptic feedback
14. Побудувати Chat UI:
    - `ChatView` — основний контейнер
    - `MessageBubbleView` — окреме повідомлення (user/assistant/system/tool)
    - `MarkdownContentView` — рендеринг markdown з підсвіткою коду
    - `CodeBlockView` — syntax highlighted code з copy button та line numbers
    - `ToolCallCardView` — inline expandable tool call card
    - `StreamingIndicatorView` — typing dots animation
    - `ConnectionStatusBar` — animated pill у header
    - `ChatInputBar` — multi-line input з кнопками
    - `QuickActionsRow` — preset buttons (Explain, Fix, Refactor, Test)
    - `ScrollToBottomFAB` — floating button з badge
15. Додати жести та інтерактивність:
    - Swipe actions на повідомленнях (copy, retry, delete)
    - Long press context menu
    - Pull to refresh → reconnect
    - Search overlay з highlighting

### Phase 5: Task Dashboard Feature
16. Реалізувати `TasksReducer`:
    - Парсинг `task.md` формату ([ ], [/], [x], [-]) в TaskNode tree
    - Real-time оновлення при отриманні нових tool_results
    - Filter/sort capabilities
17. Побудувати Task Dashboard UI:
    - `TaskDashboardView` — контейнер з header stats
    - `TaskTreeView` — ієрархічний список з expand/collapse
    - `TaskNodeRow` — рядок задачі зі status icon, title, duration
    - `TaskStatsHeader` — circular progress rings, stats cards
    - `TaskDetailSheet` — деталі задачі при tap
    - Animated transitions при зміні статусу

### Phase 6: Tools Inspector Feature
18. Реалізувати `ToolsReducer`:
    - Timeline management, filtering, statistics calculation
    - Tool call aggregation per type
19. Побудувати Tools Inspector UI:
    - `ToolsInspectorView` — контейнер
    - `ToolTimelineView` — хронологічний список
    - `ToolCallCard` — детальна картка tool call
    - `ToolFilterBar` — horizontal scroll chips
    - `ToolStatsView` — charts та statistics
    - `ToolDetailSheet` — full arguments/output view

### Phase 7: Logs/Terminal Feature
20. Реалізувати `LogsReducer`:
    - Log buffering (ring buffer, max 10K entries)
    - Level filtering, source filtering
    - Search with regex support
21. Побудувати Terminal UI:
    - `TerminalView` — terminal aesthetic container
    - `LogLineView` — individual log line з level color
    - `LogFilterBar` — level chips
    - `LogSearchBar` — search з match navigation
    - Auto-scroll logic з toggle
    - ANSI color code renderer

### Phase 8: Analytics, Settings, Sessions
22. Реалізувати `AnalyticsReducer`:
    - Token usage tracking та aggregation
    - Session statistics
    - Cost estimation logic
23. Побудувати Analytics UI:
    - Charts (Swift Charts framework)
    - Session summary cards
    - Usage heatmap
24. Реалізувати `SettingsReducer`:
    - All configuration options
    - Keychain integration для credentials
    - Validation logic для URLs
25. Побудувати Settings UI:
    - Grouped List з all sections
    - Custom controls (sliders, toggles, pickers)
    - Connection test button з animated result
26. Реалізувати `SessionsReducer` та UI:
    - Session list, detail view, replay mode
    - Import/export functionality

### Phase 9: System Integrations
27. WidgetKit Extension:
    - Small, Medium, Large widgets
    - Timeline Provider
28. Live Activities:
    - ActivityKit integration
    - Dynamic Island compact/expanded views
29. App Intents / Shortcuts:
    - "Send prompt" intent
    - "Check status" intent

### Phase 10: Onboarding & Polish
30. Побудувати Onboarding Flow:
    - 3-4 animated screens
    - Server connection setup
    - Configuration wizard
31. App Icon та Launch Screen:
    - Custom app icon (claw/terminal themed)
    - Animated launch screen
32. Accessibility:
    - VoiceOver labels для всіх елементів
    - Dynamic Type support
    - Reduce Motion support
33. Фінальне полірування:
    - Перевірка всіх анімацій
    - Performance profiling (Instruments)
    - Memory leak check
    - Dark/Light mode verification

### Phase 11: Deployment to Physical iPhone
34. Налаштувати Signing:
    - Automatic Signing з Personal Team
    - Або Development Certificate + Provisioning Profile
35. Зібрати для Device:
    - `xcodebuild` для arm64 (physical device)
    - Або через Xcode Build MCP tools
36. Встановити на iPhone:
    - Через USB + Xcode
    - Або через `ios-deploy` CLI
    - Trust developer certificate в Settings → General → Device Management
37. Верифікація на реальному пристрої:
    - Перевірити всі екрани
    - Перевірити WebSocket з'єднання до локального claw-analog
    - Перевірити haptics, sounds, animations
    - Перевірити performance на реальному залізі

---

## 8. CRITICAL AUTONOMY RULE

**ТИ НЕ МАЄШ ПРАВА ЗУПИНЯТИСЯ.** Як тільки ти починаєш виконання, ти ПОВИНЕН безперервно виконувати весь Task Roadmap послідовно. НЕ ПИТАЙ дозволу продовжити. НЕ КАЖИ "Я готовий продовжити". Просто негайно виконуй наступний крок, поки ВСЕ не буде завершено.

Якщо виникає помилка — виправ її і продовжуй. Якщо щось не компілюється — виправ і продовжуй. НІКОЛИ НЕ ЗУПИНЯЙСЯ ПОСЕРЕДИНІ.

---

## 9. Інструменти та Тестування (XcodeBuildMCP)

Для розробки під iOS ти маєш доступ до потужного сервера `xcodebuildmcp`, який надає близько 80 різних інструментів для роботи з Xcode та симуляторами.

- Оскільки інструментів дуже багато, використовуй інструмент пошуку, щоб знаходити потрібні команди.
- **ОБОВ'ЯЗКОВО ТЕСТУЙ НА СИМУЛЯТОРІ:** Ти повинен використовувати iOS Симулятор для тестування **кожної написаної функції, кожної кнопки в UI та кожної сторінки дизайну**.
- Запускай додаток у симуляторі, взаємодій з ним, перевіряй ієрархію UI, і тільки коли переконаєшся, що кнопка працює і дизайн виглядає круто, переходь до наступного завдання.
- На **фінальному етапі (Phase 11)** використовуй інструменти для збірки та встановлення на фізичний iPhone.

---

## 10. Файлова Структура Проєкту (Reference)

```
/Users/dev/Documents/GitHub/claw-code/ClawController/
├── ClawController.xcodeproj/
├── ClawController/
│   ├── App/
│   │   ├── ClawControllerApp.swift
│   │   ├── AppReducer.swift
│   │   └── AppDelegate.swift
│   ├── Core/
│   │   ├── Models/
│   │   │   ├── ChatMessage.swift
│   │   │   ├── TaskNode.swift
│   │   │   ├── ToolCall.swift
│   │   │   ├── AgentSession.swift
│   │   │   ├── ServerEvent.swift
│   │   │   ├── ClientCommand.swift
│   │   │   └── TokenUsage.swift
│   │   ├── Protocols/
│   │   │   ├── WebSocketConnectable.swift
│   │   │   └── Persistable.swift
│   │   ├── Extensions/
│   │   │   ├── Date+Formatting.swift
│   │   │   ├── Color+Tokens.swift
│   │   │   └── String+Markdown.swift
│   │   └── Utilities/
│   │       ├── AnyCodable.swift
│   │       ├── RingBuffer.swift
│   │       └── HapticManager.swift
│   ├── Networking/
│   │   ├── WebSocketManager.swift
│   │   ├── NDJSONParser.swift
│   │   ├── ConnectionStateMachine.swift
│   │   ├── NetworkMonitor.swift
│   │   └── ServerEventTypes.swift
│   ├── Features/
│   │   ├── Chat/
│   │   │   ├── ChatReducer.swift
│   │   │   ├── ChatView.swift
│   │   │   ├── MessageBubbleView.swift
│   │   │   ├── MarkdownContentView.swift
│   │   │   ├── CodeBlockView.swift
│   │   │   ├── ToolCallCardView.swift
│   │   │   ├── StreamingIndicatorView.swift
│   │   │   ├── ConnectionStatusBar.swift
│   │   │   ├── ChatInputBar.swift
│   │   │   ├── QuickActionsRow.swift
│   │   │   └── ScrollToBottomFAB.swift
│   │   ├── Tasks/
│   │   │   ├── TasksReducer.swift
│   │   │   ├── TaskDashboardView.swift
│   │   │   ├── TaskTreeView.swift
│   │   │   ├── TaskNodeRow.swift
│   │   │   ├── TaskStatsHeader.swift
│   │   │   └── TaskDetailSheet.swift
│   │   ├── Tools/
│   │   │   ├── ToolsReducer.swift
│   │   │   ├── ToolsInspectorView.swift
│   │   │   ├── ToolTimelineView.swift
│   │   │   ├── ToolCallCard.swift
│   │   │   ├── ToolFilterBar.swift
│   │   │   ├── ToolStatsView.swift
│   │   │   └── ToolDetailSheet.swift
│   │   ├── Logs/
│   │   │   ├── LogsReducer.swift
│   │   │   ├── TerminalView.swift
│   │   │   ├── LogLineView.swift
│   │   │   ├── LogFilterBar.swift
│   │   │   └── LogSearchBar.swift
│   │   ├── Analytics/
│   │   │   ├── AnalyticsReducer.swift
│   │   │   ├── AnalyticsDashboardView.swift
│   │   │   ├── TokenUsageChart.swift
│   │   │   ├── UsageHeatmap.swift
│   │   │   └── SessionSummaryCard.swift
│   │   ├── Settings/
│   │   │   ├── SettingsReducer.swift
│   │   │   ├── SettingsView.swift
│   │   │   ├── ConnectionSettingsSection.swift
│   │   │   ├── AgentConfigSection.swift
│   │   │   ├── AppearanceSection.swift
│   │   │   └── AboutSection.swift
│   │   ├── Sessions/
│   │   │   ├── SessionsReducer.swift
│   │   │   ├── SessionListView.swift
│   │   │   ├── SessionDetailView.swift
│   │   │   └── SessionReplayView.swift
│   │   └── Onboarding/
│   │       ├── OnboardingReducer.swift
│   │       ├── OnboardingView.swift
│   │       ├── WelcomeStep.swift
│   │       ├── ConnectStep.swift
│   │       ├── ConfigureStep.swift
│   │       └── ReadyStep.swift
│   ├── DesignSystem/
│   │   ├── Tokens/
│   │   │   ├── ColorTokens.swift
│   │   │   ├── TypographyTokens.swift
│   │   │   ├── SpacingTokens.swift
│   │   │   └── AnimationTokens.swift
│   │   ├── Components/
│   │   │   ├── GlassmorphicCard.swift
│   │   │   ├── GradientButton.swift
│   │   │   ├── StatusBadge.swift
│   │   │   ├── ShimmerView.swift
│   │   │   ├── PulsingDot.swift
│   │   │   ├── FloatingTabBar.swift
│   │   │   ├── SearchBar.swift
│   │   │   └── LoadingOverlay.swift
│   │   ├── Modifiers/
│   │   │   ├── GlassmorphicModifier.swift
│   │   │   ├── CardStyleModifier.swift
│   │   │   ├── HapticFeedbackModifier.swift
│   │   │   ├── StaggerAnimationModifier.swift
│   │   │   └── ShimmerModifier.swift
│   │   └── Animations/
│   │       ├── TypewriterEffect.swift
│   │       ├── ParticleEffect.swift
│   │       └── MorphingShape.swift
│   ├── Persistence/
│   │   ├── SwiftDataModels/
│   │   │   ├── PersistedSession.swift
│   │   │   └── PersistedMessage.swift
│   │   ├── KeychainService.swift
│   │   ├── SessionStorage.swift
│   │   └── SettingsStorage.swift
│   ├── SystemIntegrations/
│   │   ├── Widgets/
│   │   │   ├── ClawWidgetBundle.swift
│   │   │   ├── ConnectionStatusWidget.swift
│   │   │   └── ActiveTaskWidget.swift
│   │   ├── LiveActivities/
│   │   │   ├── RunActivityAttributes.swift
│   │   │   └── RunLiveActivityView.swift
│   │   └── AppIntents/
│   │       ├── SendPromptIntent.swift
│   │       └── CheckStatusIntent.swift
│   └── Resources/
│       ├── Assets.xcassets/
│       ├── Fonts/
│       │   └── JetBrainsMono/
│       └── Sounds/
│           └── notification.caf
└── ClawControllerTests/
    ├── Networking/
    │   ├── WebSocketManagerTests.swift
    │   └── NDJSONParserTests.swift
    ├── Features/
    │   ├── ChatReducerTests.swift
    │   ├── TasksReducerTests.swift
    │   └── ToolsReducerTests.swift
    └── Core/
        └── ModelsTests.swift
```

---

## 11. Сумісність з claw-analog Backend

iOS-клієнт спілкується з `claw-analog` у режимі `--output-format json --stream`. Сервер повертає NDJSON через WebSocket або HTTP SSE. Клієнт повинен підтримувати обидва транспорти.

### Контракт NDJSON (claw-analog):
| type | Payload |
|------|---------|
| `run_start` | `schema`, `format_version`, `workspace`, `model`, `stream`, `permission`, `preset`, `session`, `rag_enabled` |
| `turn_start` | `turn` (number) |
| `assistant_text_delta` | `delta` (string) — streaming text fragment |
| `assistant_turn` | `stop_reason`, `usage`, `text`, `tool_calls[]` |
| `tool_result` | `name`, `tool_use_id`, `is_error`, `output`, `truncated`, `output_len_chars` |
| `run_end` | `ok` (bool) |
| `error` | `message` (string) |

---

**Кінцева мета:** Після завершення всіх фаз, на iPhone користувача повинен бути встановлений повнофункціональний, красивий, анімований додаток Claw Controller, який в реальному часі підключається до claw-analog та дозволяє повністю контролювати AI-агента з телефону.

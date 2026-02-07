# Phantom v2 — Complete Feature Catalog

> **Purpose**: This document catalogs every feature in the current Phantom desktop app so a designer can understand the full scope of what must be ported to the new React/SolidJS UI.

---

## 1. Product Summary

**Phantom** is a desktop cockpit for AI coding agents. It lets developers run multiple AI agents (Codex, Claude Code, Droid, GeminiCLI, OpenCode) simultaneously from a single interface, with isolated git worktrees, unified auth, analytics, and integrations with GitHub, Linear, Sentry, and Discord.

**Current stack**: Tauri (Rust backend) + Vanilla HTML/CSS/JS + Bootstrap 5 + jQuery  
**New stack**: Tauri (Rust backend) + React/SolidJS frontend  
**Target agents (v2)**: Codex, Claude Code, Droid, GeminiCLI, OpenCode *(dropping Ampcode)*

---

## 2. Navigation Structure (Current)

The current app has a **top nav bar** with 8 tabs + settings gear:

| Tab | Internal ID | Description |
|-----|-------------|-------------|
| Create Tasks | `createTasksPage` | Agent selection, prompt input, task config |
| View Tasks | `viewTasksPage` | Task list table with status/actions |
| Accounts | `accountsPage` | OAuth login + API key management |
| Skills | `skillsPage` | SKILL.md file browser + enable/disable |
| Analytics | `analyticsPage` | Token usage, cost, model distribution charts |
| Review | `reviewPage` | Git diff viewer, commit timeline |
| Command | `commandPage` | GitHub/Linear/Sentry issue dashboard |
| Notes | `notesPage` | Meeting transcription + text notes |
| Settings | `settingsPage` | Full config surface (hidden behind gear icon) |
| Skill Tree | `skills-tree.html` | Separate window — interactive orbital skill visualization |

Plus a **global Cmd+K command palette** overlay for semantic search.

---

## 3. Feature-by-Feature Breakdown

### 3.1 Create Tasks Page

The primary task creation interface.

**Agent Selection Grid**
- Clickable agent cards with logos (grid layout)
- Each card shows availability status (installed/not installed)
- Beta badges on newer agents
- Agent availability checked on startup and updated in real-time

**Prompt Input**
- Rich contenteditable textarea (not a plain `<textarea>`)
- Slash command autocomplete (`/` trigger → fuzzy-filtered dropdown)
- Image paste support (CMD+V for inline images)
- Image attachment button (JPEG, PNG, GIF, WebP, max 5MB)
- Attachment preview thumbnails

**Task Configuration**
- **Permission Mode** dropdown: Bypass Permissions, Accept Edits, Default, Don't Ask, Plan Mode
- **Model** dropdown: Dynamic per-agent model list (static from config or fetched via API)
- **Reasoning Effort** dropdown (Codex-only): Low / Medium / High
- **Agent Mode** dropdown (agent-specific): Build, Plan, General, Explore
- **Project Path** picker: Browse button + text input + recent paths
- **Base Branch** dropdown: Lists git branches from selected project
- **Use Worktree** toggle: On/Off
- **Claude Runtime** toggle (Claude-only): Native / Docker
- **Schedule Task** button: Opens scheduler modal (cron-based automation)
- **Create Task** button: Primary action

**Multi-Agent Creation**
- Can create multiple agent sessions simultaneously
- Worktree automatically enabled for multi-agent runs

### 3.2 View Tasks Page

Task management and monitoring table.

**Global Actions Bar**
- Start All / Stop All / Delete All buttons

**Tasks Table** (sortable columns)
| Column | Description |
|--------|-------------|
| ID | Short task identifier |
| AGENT | Agent logo/icon |
| MODEL | Model name used |
| WORKTREE | Branch/worktree path |
| STATUS | Status pill with dropdown (Idle/Running/Completed/Error/Stopped) |
| CTX | Context window usage indicator |
| COST | Session cost estimate (USD) |
| ACTIONS | Start/Stop/Delete/Open Chat buttons |

**Per-Task Actions**
- Start task
- Stop task (soft stop — graceful interruption)
- Delete task (with uncommitted changes warning)
- Open chat log (opens `agent_chat_log.html`)
- Status dropdown for manual status override

### 3.3 Agent Chat Log (opens in new view/window)

Full-featured chat interface per task.

**Chat Header**
- Agent icon + task name/title
- Model badge
- Branch name display
- Diff stats counter (+/- lines)
- Git PR button (Create PR / View existing PR)
- AI code review dropdown
- Terminal toggle button
- Status bar with session cost

**Chat Messages**
- Streaming markdown rendering (with syntax highlighting via highlight.js)
- Tool call visualization with dedicated icons per tool type (Read, Write, Edit, Bash, Grep, etc.)
- Collapsible tool call details
- User message input with slash commands
- Intermediate "thinking" message filtering
- Image lightbox for attachments
- Max 300 rendered messages with pruning

**Embedded Terminal**
- PTY-based terminal (xterm.js) in a resizable drawer
- Connected to task's worktree directory
- Toggle open/close
- Auto-resize on panel resize

**PR Creation Flow**
- PR readiness check (uncommitted changes, upstream status)
- Existing PR detection with cache
- Create PR button → generates PR via gh CLI

**Code Review**
- AI-powered code review dropdown
- Diff context sent to agent for review

### 3.4 Accounts Page

Authentication management for agent providers.

**Codex (ChatGPT) Card**
- OAuth "Sign in with ChatGPT" button
- Multiple account support with add/switch
- Account list showing email, plan type, auth status
- API key input (OPENAI_API_KEY)
- Usage tracker: Session (5h window) + Weekly (7d window) percentage bars

**Claude Code Card**
- OAuth "Sign in to Claude Code" button
- API key input (ANTHROPIC_API_KEY)
- Keychain permission prompt for usage tracking
- Usage tracker: Session + Weekly percentage bars
- Email display with privacy redaction toggle

**General**
- Save Credentials button
- Keys stored locally in app settings
- Auth status indicators (Connected/Not connected)

### 3.5 Skills Page

Manages SKILL.md instruction files that extend agent capabilities.

**Skills Browser**
- Agent-tabbed view (switch between agents)
- Two categories: Personal skills (user-level) + Project skills (repo-level)
- Each skill shows: name, source path, enabled/disabled status
- Toggle to enable/disable individual skills
- Refresh button to rescan filesystem
- Opens Skill Tree view (separate window)

**Skill Tree View** (separate `skills-tree.html`)
- Interactive orbital/constellation visualization
- Particle effects and animations
- Skill nodes with icons, labels, category indicators
- Zoom controls and pan navigation
- Agent switcher buttons
- XP bar (gamification element)
- Legend for personal/project/locked skills
- Tooltips on hover
- Apply changes button for batch enable/disable
- Game-inspired dark UI aesthetic

### 3.6 Analytics Page (formerly "Command Center")

Usage analytics and cost tracking dashboard.

**Token Tracking**
- Per-agent token usage charts
- Input vs output token breakdown
- Daily usage charts
- Cache efficiency metrics

**Cost Estimates**
- Per-task cost calculation based on model pricing
- Model pricing table (per million tokens for input/output)
- Aggregate session costs

**Model Distribution**
- Which models are used most
- Agent-by-agent comparison

**Head-to-Head Comparison**
- Agent performance leaderboard
- Comparative metrics

### 3.7 Review Page

Git diff viewer and code review center.

**Project Selector**
- Dropdown with all projects that have active worktrees

**Task Selector**
- Filter tasks by project

**File Browser**
- Changed files list with diff indicators (+/- per file)
- File selection for diff viewing

**Diff Viewer**
- Split view mode (side-by-side)
- Unified view mode
- Compare against: main branch or other branches

**Commit Timeline**
- Visual timeline of commits
- Commit messages and metadata

**Actions**
- Open worktree in Finder/editor

### 3.8 Command Page (Issue Dashboard)

Aggregates developer tasks from external tools.

**GitHub Issues Panel**
- Shows issues from watched repositories
- Labels with color coding
- CI/CD status (build pass/fail indicators)
- Re-run failed workflow action
- Time-ago formatting

**Linear Issues Panel**
- Priority-based issue list
- Priority class badges (Urgent/High/Medium/Low)
- Cycle filtering

**Sentry Errors Panel**
- Error list with count and criticality
- Critical error highlighting
- Resolve error action

**Quick Actions**
- Fix Top Error → creates a task with the error context
- Start Next Issue → creates a task from the top Linear issue
- Re-run Failed CI → triggers workflow re-run

**Agent Popup**
- Quick task creation modal from any issue/error
- Agent selection + prompt pre-fill

**Auto-refresh**
- Configurable refresh interval (default 15 min)
- Skeleton loading states

### 3.9 Notes Page

Meeting notes, text notes, transcription, and dictation hub.

**Recording Controls**
- Start/Pause/Resume/Stop buttons
- Timer display
- Session title input

**Transcript View**
- Real-time transcription segments (timestamped)
- Segment editing
- Export (TXT/MD/JSON formats)

**Sessions Sidebar**
- Session list grouped by date
- Folder organization with drag-to-move
- Search (semantic-first with title-substring fallback)
- Session delete/rename

**Text Notes**
- Rich text note creation (non-audio)
- Inline note save

**AI Chat Sidebar**
- Embedded agent chat per note/session
- Template-driven prompts (e.g., "Summarize meeting", "Action items")
- Agent and thread selector

**Templates**
- Prompt preset management (CRUD)
- Chips for quick template selection
- Template editor modal
- Default templates (Meeting Summary, Action Items, etc.)

**Calendar Integration**
- Apple Calendar (EventKit) integration
- Upcoming events panel
- Calendar selection settings
- Auto-refresh of upcoming events

**Model Management**
- Whisper model download/management (multiple sizes)
- Parakeet model support (NVIDIA ONNX-based ASR)
- Model selector dropdown
- Download progress with cancel
- Size display for each model

**Dictation (Global)**
- System-wide voice-to-text
- Activation modes: Fn Hold (macOS), Fn Double-Press, Global Shortcut
- Live transcription preview
- Transcription engines: Local (Whisper/Parakeet) or ChatGPT
- Paste-into-inputs mode (accessibility permission required)
- Clipboard fallback
- Filler word cleanup (um, uh, like)
- Permission modals for macOS

**Settings Tab (within Notes)**
- Transcription engine selection
- Dictation activation configuration
- Model download management
- Calendar toggle

### 3.10 Settings Page

Global application configuration.

**Sections include:**

| Setting Group | Key Settings |
|---------------|-------------|
| Discord Bot | Enable/disable, Bot token, Channel ID, Test button |
| Notifications | Enable/disable, Stack mode, Timeout (0–60s) |
| AI Summaries | Enable/disable, Dedicated agent selector |
| Webhook | URL for external notifications |
| Delays | Retry delay, Error delay |
| Codex Settings | Custom CLI path, Access/Permission mode, Feature flags (Collaboration Modes, Steer, Unified Exec, Collab, Apps), Personality |
| Claude Settings | Auth method, Docker image override |
| MCP Server | Enable/disable, Port (default 43778), Bearer token |
| Project Allowlist | Multi-project path management with star/default |
| Calendar | Apple Calendar toggle + calendar picker |

---

## 4. Backend Features (Invisible to UI but Required)

These features run server-side (Rust/Tauri) and the UI must support them:

| Feature | Description |
|---------|-------------|
| Agent Process Management | Spawn/supervise up to 5 concurrent agent subprocesses (JSON-RPC over stdio) |
| Git Worktree Manager | Create isolated worktrees per task with auto-naming |
| SQLite Persistence | Tasks, sessions, messages, meeting sessions/segments, automations |
| Crash Recovery / Resume | Auto-resume incomplete sessions on app restart |
| Semantic Search | Hybrid FTS + vector rerank with local embeddings (MiniLM/BGE ONNX) |
| Incremental Indexing | Debounced re-indexing on task/message/meeting changes |
| Local Whisper / Parakeet | On-device speech-to-text models with download-on-demand |
| Name Generation | AI-powered task name generation using Anthropic/Claude |
| Claude Usage Watcher | Real-time cost tracking for Claude sessions |
| Codex Usage Watcher | Session/weekly rate limit monitoring |
| Task Automations | Cron-based scheduled task creation |
| MCP Server | Local Model Context Protocol server for external client control |
| Discord Bot | Two-way chat (threads per task, Discord → Phantom → Agent) |
| Webhook | POST notifications on task events |
| Auto-Updater | Built-in update check + install |

---

## 5. Data Model Overview

```
tasks ──────── sessions ──────── messages
                  │                    │
                  ├── tool_calls       │
                  ├── checkpoints      │
                  └── model/cost info  │
                                       │
meeting_sessions ─── meeting_segments ─┘
                                       
semantic_chunks ─── semantic_fts (FTS5)

automations (cron jobs)
settings (JSON file)
attachments (file system)
disabled-skills/<agent>/ (marker files)
```

---

## 6. Agent Capabilities Matrix

| Capability | Codex | Claude Code | Droid | GeminiCLI | OpenCode |
|-----------|-------|-------------|-------|-----------|----------|
| OAuth Login | ✅ (ChatGPT) | ✅ (Anthropic) | ❌ | ❌ | ❌ |
| API Key | ✅ | ✅ | ❌ | ✅ | ✅ |
| Plan Mode | ✅ | ✅ | ✅ | TBD | ✅ |
| Model Selection | Dynamic (API) | Static config | Static config | TBD | Static config |
| Reasoning Effort | ✅ | ❌ | ❌ | TBD | ❌ |
| Agent Modes | ❌ | ❌ | ❌ | TBD | ✅ (build/plan/general/explore) |
| Docker Runtime | ❌ | ✅ | ❌ | ❌ | ❌ |
| Slash Commands | ✅ (ACP) | ✅ (ACP) | ❌ | TBD | ❌ |
| Usage Tracking | ✅ (session/weekly) | ✅ (session/weekly) | ❌ | TBD | ❌ |
| Multiple Accounts | ✅ | ❌ | ❌ | ❌ | ❌ |

> **Note**: GeminiCLI is new for v2 and needs to be wired in. TBD items require discovery.

---

## 7. Keyboard Shortcuts (Current)

| Shortcut | Action |
|----------|--------|
| Cmd+K | Global command palette (semantic search) |
| Cmd+V | Paste image into prompt |
| / (in prompt) | Trigger slash command autocomplete |
| Tab navigation | Switch between Create/View/Accounts/etc. tabs |

---

## 8. Notifications System

| Channel | Description |
|---------|-------------|
| Native OS | macOS notifications when agents need attention |
| Discord | Thread-based notifications per task |
| Webhook | POST to configurable URL |
| In-App Badge | Usage warning badge for near-limit agents |
| Sound | Checkout sound effect on task completion |

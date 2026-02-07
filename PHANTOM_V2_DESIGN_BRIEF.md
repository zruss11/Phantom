# Phantom v2 — Design Brief & Direction

> **For**: UI/UX Designer (or design-focused LLM agent)  
> **From**: Product team  
> **Date**: February 2026  
> **Companion doc**: `phantom_v2_feature_catalog.md` (exhaustive feature reference)

---

## 1. Project Context

### What Is Phantom?
Phantom is a desktop app that lets developers run multiple AI coding agents (Codex, Claude Code, Droid, GeminiCLI, OpenCode) from a single cockpit. Think of it as a "mission control" for AI-assisted software development.

### Why Redesign?
The current UI was built as a quick Bootstrap/jQuery prototype inspired by sneaker bot UIs. It has outgrown this aesthetic:
- **Not B2B SaaS-friendly** — looks like a consumer tool, not enterprise software
- **Tab-based navigation doesn't scale** — 8+ tabs across the top is cramped
- **No auth/onboarding flow** — there's no login, team management, or account setup
- **No theming** — currently dark-only with a pink/gaming aesthetic
- **Monolithic HTML** — the single `menu.html` is 3100+ lines

### What's Changing
| Aspect | Current | New |
|--------|---------|-----|
| Framework | Vanilla JS + jQuery + Bootstrap | React + SolidJS |
| Navigation | Top tab bar (8 tabs) | Sidebar-based (Notion-style) |
| Auth | None (desktop-only) | WorkOS login + account setup + teams |
| Theming | Dark-only, pink accents | Dark + Light mode |
| Agents | Codex, Claude, Amp, Droid, OpenCode | Codex, Claude, Droid, GeminiCLI, OpenCode |
| Aesthetic | Gaming/sneaker bot | Clean, enterprise, Notion-inspired |

---

## 2. Design Direction

### Primary Inspiration: Notion
The UX should feel like **Notion** — clean, spacious, content-first, with a calm professionalism. Key principles to inherit:

1. **Sidebar navigation** — collapsible, icon+label, nested sections
2. **Content-first layouts** — generous white space, clear visual hierarchy
3. **Subtle interactions** — hover states, smooth transitions, no flashy animations
4. **Typography-driven** — clean sans-serif (Inter, Geist, or similar), clear heading hierarchy
5. **Minimal chrome** — reduce borders, cards, and visual containers
6. **Inline editing** — click-to-edit where possible (task names, notes)
7. **Command palette** — Cmd+K is central to the experience (already built)

### Secondary Inspirations
- **Linear** — for issue/task management UX and keyboard-first navigation
- **Vercel Dashboard** — for analytics/metrics presentation
- **Raycast** — for command palette and search experience
- **GitHub Copilot Chat** — for agent chat interface patterns

### What NOT to Do
- ❌ No gaming/sneaker bot aesthetic (gradient cards, particle effects, XP bars)
- ❌ No bright pink branding — shift to a more neutral, professional palette
- ❌ No Bootstrap-style card grids — use content blocks with subtle separators
- ❌ No cluttered toolbars — progressive disclosure instead

---

## 3. Color System

### Light Mode (Primary)
```
Background:       #FFFFFF (page), #F7F7F5 (sidebar/surface)
Text:              #37352F (primary), #787774 (secondary), #B4B4B0 (tertiary)
Borders:           #E9E9E7 (subtle), #DFDFDD (stronger)
Accent:            #2383E2 (primary blue — links, active states)
Success:           #0F7B0F
Warning:           #D9730D  
Error:             #E03E3E
```

### Dark Mode
```
Background:        #191919 (page), #202020 (sidebar/surface)
Text:              #E7E7E5 (primary), #9B9A97 (secondary), #5A5A58 (tertiary)
Borders:           #2F2F2F (subtle), #373737 (stronger)
Accent:            #529CCA (primary blue — slightly muted)
Success:           #4DAB9A
Warning:           #E9983D
Error:             #EB5757
```

### Agent Colors (for badges, charts, avatars)
| Agent | Color |
|-------|-------|
| Codex | `#10A37F` (OpenAI green) |
| Claude Code | `#D4A574` (Anthropic amber) |
| Droid | `#6366F1` (Factory purple) |
| GeminiCLI | `#4285F4` (Google blue) |
| OpenCode | `#F59E0B` (OpenCode amber) |

---

## 4. Information Architecture (New)

Replace the top tab bar with a **collapsible sidebar**:

```
┌─────────────────────────────────────────────────┐
│ SIDEBAR              │  MAIN CONTENT AREA       │
│                      │                          │
│ 🔍 Search (Cmd+K)   │                          │
│                      │                          │
│ ─── WORKSPACE ───    │                          │
│ + New Task           │                          │
│ 📋 Tasks             │                          │
│ 📊 Analytics         │                          │
│ 🔀 Review            │                          │
│                      │                          │
│ ─── INTEGRATIONS ─── │                          │
│ 🐙 GitHub            │                          │
│ 📐 Linear            │                          │
│ 🐛 Sentry            │                          │
│                      │                          │
│ ─── TOOLS ─────────  │                          │
│ 🎙 Notes & Meetings  │                          │
│ ⚡ Skills            │                          │
│ ⌨️  Terminal          │                          │
│                      │                          │
│ ─── bottom ────────  │                          │
│ 🔑 Accounts          │                          │
│ ⚙️  Settings          │                          │
│ 👤 Profile/Logout    │                          │
└─────────────────────────────────────────────────┘
```

### Key IA Changes
1. **"Create Tasks" and "View Tasks" merge** → single "Tasks" view with inline creation
2. **"Command" page splits** → GitHub, Linear, Sentry become separate sidebar items under "Integrations"
3. **Login/Profile added** → bottom of sidebar
4. **Settings remain** → but organized into clear sub-pages
5. **Terminal** → promoted to sidebar item (currently buried in chat view)

---

## 5. Page-by-Page Design Requirements

### 5.1 Login & Onboarding (NEW)

> Currently doesn't exist — must be designed from scratch.
> **Auth provider: [WorkOS](https://workos.com)** — enterprise-grade auth with SSO, directory sync, and MFA.

**Login Page**
- Email + password login (via WorkOS AuthKit)
- SSO / OAuth options (Google, GitHub, enterprise SAML)
- "Forgot password" flow
- Clean, centered layout with Phantom logo
- MFA challenge screen (TOTP / SMS)

**Onboarding Flow** (first-time)
1. Welcome screen + value prop
2. Create or join a workspace
3. Connect agent accounts (Codex OAuth, Claude OAuth)
4. Select a project directory
5. Install agents check (detect which CLIs are available)
6. Create first task

**Account Setup**
- Profile: name, email, avatar
- Workspace management
- Subscription/plan display

**Team Management** (invite + roles)
- Invite members by email
- Role assignment: Owner, Admin, Member
- Pending invite list with resend/revoke
- Member list with role badges and last-active timestamp
- Remove member action (with confirmation)

---

### 5.2 Tasks (Merged Create + View)

**Layout**: Split view — task list (left 35%) + task detail/chat (right 65%)

**Task List Panel**
- Scrollable list of tasks, most recent first
- Each row: agent icon, title (AI-summarized), model badge, status indicator, cost, time ago
- Status indicators: ● Running (green pulse), ● Completed (green solid), ● Error (red), ● Stopped (gray), ● Idle (blue)
- Filter/sort controls: by agent, by status, by project
- Inline "New Task" button at top → expands into creation form within the list panel

**Task Creation Form** (inline, not a separate page)
- Agent selector (horizontal pill-style, not a grid of large cards)
- Prompt textarea with slash command autocomplete
- Image paste/attach
- Collapsible "Advanced" section: model, permission mode, reasoning effort, base branch, worktree toggle, runtime
- "Create" button

**Task Detail / Chat Panel**
- Header: task title (editable), agent badge, model, branch, diff stats, PR button
- Scrolling chat log with markdown rendering
- Tool calls as collapsible accordion items with icons
- Message input at bottom with slash commands
- Cost display in footer
- Terminal drawer (slide-up panel)

---

### 5.3 Analytics

**Layout**: Dashboard grid with metric cards

**Metric Cards**
- Total tokens used (with sparkline)
- Total cost estimate
- Active tasks count
- Agent distribution (donut chart)

**Charts Section**
- Daily usage over time (area chart)
- Cost by agent (stacked bar)
- Model usage distribution (horizontal bar)
- Cache efficiency (for agents that report cache hits)

**Design Notes**
- Use Vercel-style dark metric cards with large numbers
- Subtle grid lines, no heavy borders
- Time range selector (24h / 7d / 30d)

---

### 5.4 Review

**Layout**: Three-column — file tree (left) + diff view (center) + commit timeline (right)

- Project/task selector at top
- File tree with change indicators (M/A/D)
- Split diff view (default) with toggle to unified
- Commit timeline with messages
- "Open in Editor" action

---

### 5.5 Integrations (GitHub, Linear, Sentry)

Each integration gets its own sidebar page, same layout pattern:

**Common Layout**
- Header with connection status + config link
- Issue/error list with priority/severity badges
- "Create Task from Issue" action (opens task creation with pre-filled prompt)
- Auto-refresh indicator

**GitHub-specific**: Issues + CI/CD workflow status  
**Linear-specific**: Issues with priority + cycle  
**Sentry-specific**: Errors with event count + criticality

---

### 5.6 Notes & Meetings

**Layout**: Three-panel — sidebar (sessions list, left) + content (center) + AI chat (right, toggle-able)

**Sessions Sidebar**
- Folder organization
- Session list with icons (🎙 recording, 📝 text note)
- Calendar integration ("Coming Up" events)
- Search bar with semantic search

**Content Area**
- For recordings: transcript with timestamps, recording controls (record/pause/stop/timer)
- For text notes: rich text editor
- Template chips for quick AI actions
- Export button (TXT/MD/JSON)

**AI Chat Panel** (slide-in from right)
- Embedded agent chat for the current note
- Template-driven prompts
- Agent + model selector

**Model Management**
- Settings within Notes for downloading/managing Whisper + Parakeet models
- Download progress bars

---

### 5.7 Skills

**Layout**: List/grid view with agent tabs

- Agent tab bar at top (filter skills by agent)
- Skill cards: name, source indicator (personal/project), enabled toggle, file path
- Skill Tree button → opens interactive visualization
  - **Design note**: The current Skill Tree is gamified (XP bars, particles). For v2, consider simplifying to a clean tree/graph view (e.g., react-flow style), keeping it visually interesting but less "game UI"

---

### 5.8 Settings

**Layout**: Sidebar sub-navigation (like Notion settings)

**Sub-pages**:
1. **General**: app behavior, delays, AI summaries
2. **Agents**: per-agent config (Codex path, feature flags, personality, Claude Docker image)
3. **Integrations**: Discord bot, MCP server, Webhooks
4. **Project Allowlist**: manage allowed project paths
5. **Appearance**: dark/light mode toggle, font size, density
6. **About**: version info, update check, changelog

---

### 5.9 Global Command Palette (Cmd+K)

**Behavior**: Full-screen modal overlay — Notion/Linear style

- Search input with "Exact" toggle
- Results grouped by type: Tasks, Notes, Commands
- Keyboard navigation (↑↓ to select, Enter to open)
- Recent items when empty
- Fuzzy matching
- Indexing status indicator

---

## 6. Component Library

### Foundation Components
| Component | Notes |
|-----------|-------|
| Button | Primary (filled), Secondary (outline), Ghost, Danger variants |
| Input | Standard, Password (with toggle), Search, TextArea, ContentEditable |
| Toggle | On/Off switch for settings |
| Dropdown / Select | Custom dropdown with descriptions per option (see current `custom-dropdown.js`) |
| Badge | Status (Running/Completed/Error), Agent (with color), Beta |
| Avatar | Agent icons (SVG/PNG), User avatar |
| Card | Metric card, List item card |
| Modal | Confirmation, Form, Lightbox |
| Tab Bar | Horizontal pills for agent/category switching |
| Tooltip | On hover, positioned auto |
| Toast | Success/Error/Info notifications |
| Progress Bar | Deterministic (model downloads, usage meters) |
| Skeleton Loader | For async data loading |
| Command Palette | Full-screen search overlay |
| Collapsible / Accordion | For tool calls in chat, advanced settings |
| Diff Viewer | Split + Unified modes |
| Terminal | xterm.js integration |
| Markdown Renderer | With syntax highlighting, image lightbox |

### Agent Branding
Each agent needs a consistent visual identity:
- Icon/logo (SVG, transparent)
- Brand color (see §3)
- Name badge component

---

## 7. Responsive / Window Behavior

This is a **desktop-only** Tauri app (not a web app), but windows can be resized:

- **Minimum window size**: ~1000×600px
- **Sidebar**: collapsible to icon-only (64px) or expanded (240px)
- **Chat panel**: fill available space with scrollable content
- **Terminal drawer**: resizable height
- **Skill Tree**: separate window, full-screen feel with zoom/pan

---

## 8. Dark / Light Mode Tokens

The design system must be built on CSS custom properties (or a theme context) so all colors are swappable:

```css
/* Example token structure */
--color-bg-primary:      /* Page background */
--color-bg-secondary:    /* Sidebar, cards */
--color-bg-hover:        /* Hover states */
--color-text-primary:    /* Main body text */
--color-text-secondary:  /* Muted/helper text */
--color-text-tertiary:   /* Placeholder text */
--color-border:          /* Subtle borders */
--color-border-strong:   /* Stronger borders */
--color-accent:          /* Primary interactive color */
--color-success:
--color-warning:
--color-error:
--color-agent-codex:
--color-agent-claude:
--color-agent-droid:
--color-agent-gemini:
--color-agent-opencode:
```

---

## 9. New Features to Design (Not in Current App)

| Feature | Description |
|---------|-------------|
| Login / Sign-up | WorkOS AuthKit — email + password, SSO, Google/GitHub OAuth, MFA |
| User Profile | Name, email, avatar, password change |
| Team / Workspace | Invite by email, roles (Owner/Admin/Member), member list, remove |
| GeminiCLI Agent | New agent card + any GeminiCLI-specific config |
| Light Mode | Full light mode variant for all screens |
| Onboarding | First-run wizard: workspace → agents → project → first task |
| Breadcrumbs | Show location in sidebar hierarchy |
| Activity Feed | Recent actions/events across all agents (optional) |

---

## 10. Deliverables Expected from Designer

1. **Design System**: Color tokens, typography scale, spacing system, component library
2. **Wireframes**: Lo-fi wireframes for all pages listed in §5
3. **High-Fidelity Mockups**: For key screens — Tasks (split view), Chat, Analytics, Notes
4. **Dark + Light Mode**: Both variants for all mockups
5. **Interaction Specs**: Hover states, transitions, loading states, empty states
6. **Icon Set**: Consistent icon style for sidebar nav, settings, agent actions

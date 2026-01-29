/**
 * Slash Commands Data
 * Command definitions for each ACP agent
 * These match the commands available in each agent's CLI
 */
window.SlashCommandsData = {
  'claude-code': [
    // Core commands
    { name: '/help', description: 'Show available commands' },
    { name: '/status', description: 'Show session status' },
    { name: '/clear', description: 'Clear conversation' },
    { name: '/compact', description: 'Clear history, keeping summary' },
    { name: '/config', description: 'View/modify configuration' },
    { name: '/cost', description: 'Show session costs' },
    { name: '/doctor', description: 'Diagnose issues' },
    { name: '/init', description: 'Initialize CLAUDE.md' },
    { name: '/login', description: 'Sign in to Claude' },
    { name: '/logout', description: 'Sign out of Claude' },
    { name: '/model', description: 'Switch or view model' },
    { name: '/resume', description: 'Resume previous session' },

    // Context & memory
    { name: '/add-dir', description: 'Add directory to context' },
    { name: '/context', description: 'Show/modify context window' },
    { name: '/memory', description: 'View/edit persistent memory' },
    { name: '/permissions', description: 'Manage tool permissions' },

    // Tools & features
    { name: '/bug', description: 'Report a bug' },
    { name: '/diff', description: 'Show pending changes' },
    { name: '/mcp', description: 'MCP server commands' },
    { name: '/review', description: 'Review code changes' },
    { name: '/terminal', description: 'Run terminal command' },
    { name: '/vim', description: 'Toggle vim mode' },
    { name: '/web', description: 'Search the web' },

    // Skills/extensions (common ones from plugins)
    { name: '/agent-native-audit', description: 'Run agent-native architecture review' },
    { name: '/brainstorm', description: 'Explore approaches before implementing' },
    { name: '/changelog', description: 'Create changelog for recent merges' },
    { name: '/commit', description: 'Create a git commit' },
    { name: '/create-agent-skill', description: 'Create or edit Claude Code skills' },
    { name: '/deepen-plan', description: 'Enhance plan with research' },
    { name: '/deploy-docs', description: 'Deploy documentation' },
    { name: '/fix-build', description: 'Fix failing build iteratively' },
    { name: '/hookify', description: 'Create hooks from conversation' },
    { name: '/lfg', description: 'Full autonomous engineering workflow' },
    { name: '/lint', description: 'Run linting and code quality checks' },
    { name: '/plan', description: 'Transform feature into project plan' },
    { name: '/pr', description: 'Create a pull request' },
    { name: '/release-docs', description: 'Build documentation site' },
    { name: '/reproduce-bug', description: 'Reproduce and investigate a bug' },
    { name: '/review-branch', description: 'Review current branch changes' },
    { name: '/review-commit', description: 'Review specific commit' },
    { name: '/review-pr', description: 'Review pull request' },
    { name: '/review-pr-comments', description: 'Implement PR review comments' },
    { name: '/triage', description: 'Triage and categorize findings' },
    { name: '/undo', description: 'Undo last change' },
    { name: '/work', description: 'Execute work plans' }
  ],
  'codex': [
    { name: '/model', description: 'Choose model and reasoning effort', scope: 'global' },
    { name: '/approvals', description: 'Configure approval policy', scope: 'global' },
    { name: '/setup-elevated-sandbox', description: 'Set up elevated agent sandbox', scope: 'global' },
    { name: '/experimental', description: 'Toggle beta features', scope: 'global' },
    { name: '/skills', description: 'Use skills to improve task performance', scope: 'global' },
    { name: '/review', description: 'Review current changes and find issues', scope: 'global' },
    { name: '/new', description: 'Start a new chat during a conversation', scope: 'global' },
    { name: '/resume', description: 'Resume a saved chat', scope: 'global' },
    { name: '/fork', description: 'Fork the current chat', scope: 'global' },
    { name: '/init', description: 'Create an AGENTS.md file', scope: 'global' },
    { name: '/compact', description: 'Summarize conversation to prevent context limit', scope: 'global' },
    { name: '/collab', description: 'Change collaboration mode', scope: 'global' },
    { name: '/diff', description: 'Show git diff (including untracked files)', scope: 'global' },
    { name: '/mention', description: 'Mention a file', scope: 'global' },
    { name: '/status', description: 'Show session configuration and token usage', scope: 'global' },
    { name: '/mcp', description: 'List configured MCP tools', scope: 'global' },
    { name: '/logout', description: 'Log out of Codex', scope: 'global' },
    { name: '/quit', description: 'Exit Codex', scope: 'global' },
    { name: '/exit', description: 'Exit Codex', scope: 'global' },
    { name: '/feedback', description: 'Send logs to maintainers', scope: 'global' },
    { name: '/ps', description: 'List background terminals', scope: 'global' }
  ],
  'factory-droid': [
    { name: '/help', description: 'Show available commands' },
    { name: '/status', description: 'Show session status' },
    { name: '/config', description: 'View configuration' }
  ]
};

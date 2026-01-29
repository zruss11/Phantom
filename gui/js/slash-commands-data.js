/**
 * Slash Commands Data
 * Command definitions for each ACP agent
 * These match the commands available in each agent's CLI
 */
window.SlashCommandsData = {
  'claude-code': [
    // Core commands
    { name: '/help', description: 'Show available commands', scope: 'global' },
    { name: '/status', description: 'Show session status', scope: 'global' },
    { name: '/clear', description: 'Clear conversation', scope: 'global' },
    { name: '/compact', description: 'Clear history, keeping summary', scope: 'global' },
    { name: '/config', description: 'View/modify configuration', scope: 'global' },
    { name: '/cost', description: 'Show session costs', scope: 'global' },
    { name: '/doctor', description: 'Diagnose issues', scope: 'global' },
    { name: '/init', description: 'Initialize CLAUDE.md', scope: 'global' },
    { name: '/login', description: 'Sign in to Claude', scope: 'global' },
    { name: '/logout', description: 'Sign out of Claude', scope: 'global' },
    { name: '/model', description: 'Switch or view model', scope: 'global' },
    { name: '/resume', description: 'Resume previous session', scope: 'global' },

    // Context & memory
    { name: '/add-dir', description: 'Add directory to context', scope: 'global' },
    { name: '/context', description: 'Show/modify context window', scope: 'global' },
    { name: '/memory', description: 'View/edit persistent memory', scope: 'global' },
    { name: '/permissions', description: 'Manage tool permissions', scope: 'global' },

    // Tools & features
    { name: '/bug', description: 'Report a bug', scope: 'global' },
    { name: '/diff', description: 'Show pending changes', scope: 'global' },
    { name: '/mcp', description: 'MCP server commands', scope: 'global' },
    { name: '/review', description: 'Review code changes', scope: 'global' },
    { name: '/terminal', description: 'Run terminal command', scope: 'global' },
    { name: '/vim', description: 'Toggle vim mode', scope: 'global' },
    { name: '/web', description: 'Search the web', scope: 'global' }
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

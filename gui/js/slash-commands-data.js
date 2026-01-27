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
    // Core commands
    { name: '/help', description: 'Show available commands' },
    { name: '/status', description: 'Show session status' },
    { name: '/clear', description: 'Clear conversation' },
    { name: '/compact', description: 'Compact history' },
    { name: '/exit', description: 'Exit Codex session' },
    { name: '/history', description: 'Show conversation history' },
    { name: '/model', description: 'Switch model' },
    { name: '/new', description: 'Start new conversation' },
    { name: '/quiet', description: 'Toggle quiet mode' },
    { name: '/resume', description: 'Resume previous session' },
    { name: '/save', description: 'Save current session' },

    // Approvals & permissions
    { name: '/approvals', description: 'Toggle approval mode' },

    // Files & code
    { name: '/diff', description: 'Show pending changes' },
    { name: '/mention', description: 'Reference a file' },
    { name: '/review', description: 'Review changes' },
    { name: '/undo', description: 'Undo last change' },

    // MCP & integrations
    { name: '/mcp', description: 'MCP server commands' },

    // Skills (if using with skills)
    { name: '/agent-native-audit', description: 'Run agent-native architecture review' },
    { name: '/brainstorm', description: 'Explore approaches before implementing' },
    { name: '/changelog', description: 'Create changelog for recent merges' },
    { name: '/commit', description: 'Create a git commit' },
    { name: '/create-agent-skill', description: 'Create or edit skills' },
    { name: '/deepen-plan', description: 'Enhance plan with research' },
    { name: '/deploy-docs', description: 'Deploy documentation' },
    { name: '/fix-build', description: 'Fix failing build iteratively' },
    { name: '/init', description: 'Initialize project' },
    { name: '/lfg', description: 'Full autonomous engineering workflow' },
    { name: '/lint', description: 'Run linting and code quality checks' },
    { name: '/logout', description: 'Sign out' },
    { name: '/plan', description: 'Transform feature into project plan' },
    { name: '/pr', description: 'Create a pull request' },
    { name: '/release-docs', description: 'Build documentation site' },
    { name: '/reproduce-bug', description: 'Reproduce and investigate a bug' },
    { name: '/review-branch', description: 'Review current branch changes' },
    { name: '/review-commit', description: 'Review specific commit' },
    { name: '/review-pr', description: 'Review pull request' },
    { name: '/review-pr-comments', description: 'Implement PR review comments' },
    { name: '/triage', description: 'Triage and categorize findings' },
    { name: '/work', description: 'Execute work plans' }
  ],
  'factory-droid': [
    { name: '/help', description: 'Show available commands' },
    { name: '/status', description: 'Show session status' },
    { name: '/config', description: 'View configuration' }
  ]
};

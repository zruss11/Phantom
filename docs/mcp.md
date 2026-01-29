# MCP (Model Context Protocol)

Phantom exposes a **local MCP server** while the app is running. This lets external clients (Claude Desktop, Cursor, etc.) drive Phantom tasks without a remote server.

## Settings

Open **Settings → MCP Server** to configure:

- **Enable MCP** (on/off)
- **Local MCP Port** (default: 43778)
- **MCP Token** (Bearer token for auth)

Changes take effect the next time Phantom launches.

## Endpoints

- **SSE:** `http://127.0.0.1:<PORT>/sse`
- **HTTP:** `http://127.0.0.1:<PORT>/mcp`

Auth header:

```
Authorization: Bearer <MCP_TOKEN>
```

## Tools

Superset-style tools that *remain* for Phantom:

- `create_task`
- `update_task`
- `list_tasks`
- `get_task`
- `delete_task`
- `list_task_statuses`
- `create_workspace`
- `switch_workspace`
- `delete_workspace`
- `list_workspaces`
- `navigate_to_workspace`

Phantom-specific tools:

- `phantom_start_task`
- `phantom_stop_task`
- `phantom_soft_stop_task`
- `phantom_send_chat_message`
- `phantom_get_task_history`
- `phantom_list_agents`

## Task options

When calling `create_task`, you can control worktree usage with:

- `use_worktree: true|false` (defaults to Phantom settings when omitted)

## Example (HTTP)

```json
{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}
```

Example task creation without a worktree:

```json
{
  "jsonrpc":"2.0",
  "id":2,
  "method":"tools/call",
  "params":{
    "name":"create_task",
    "arguments":{
      "prompt":"Run without a worktree",
      "use_worktree":false,
      "project_path":"/path/to/project"
    }
  }
}
```

POST to `http://127.0.0.1:<PORT>/mcp` with the Authorization header.

## Notes

- Phantom must be open for MCP to be available.
- MCP is **local only** (loopback bind).
- Task creation honors the project allowlist when configured.

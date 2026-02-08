import { useState, useEffect } from 'react';
import { ChevronDown, ChevronUp, Loader2, X } from 'lucide-react';
import clsx from 'clsx';
import { tauriInvoke } from '../../lib/tauri';
import { useTasksStore } from '../../stores/tasks';
import { AGENTS, AGENT_IDS } from '../../lib/constants';
import type { AgentId, EnrichedModel, CreateAgentPayload, CreateAgentResult } from '../../lib/types';

export function TaskCreationForm() {
  const setCreatingTask = useTasksStore((s) => s.setCreatingTask);
  const selectTask = useTasksStore((s) => s.selectTask);

  const [agentId, setAgentId] = useState<AgentId>('claude-code');
  const [prompt, setPrompt] = useState('');
  const [projectPath, setProjectPath] = useState('');
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [models, setModels] = useState<EnrichedModel[]>([]);
  const [selectedModel, setSelectedModel] = useState('');
  const [permissionMode, setPermissionMode] = useState('bypassPermissions');
  const [useWorktree, setUseWorktree] = useState(false);
  const [baseBranch, setBaseBranch] = useState('');
  const [claudeRuntime, setClaudeRuntime] = useState('native');
  const [submitting, setSubmitting] = useState(false);

  // Load models when agent changes
  useEffect(() => {
    tauriInvoke<EnrichedModel[]>('get_enriched_models', { agentId })
      .then((result) => {
        setModels(result || []);
        const defaultModel = result?.find((m) => m.isDefault);
        setSelectedModel(defaultModel?.value || result?.[0]?.value || '');
      })
      .catch(() => setModels([]));
  }, [agentId]);

  const handleSubmit = async () => {
    if (!prompt.trim() || submitting) return;

    setSubmitting(true);
    try {
      const payload: CreateAgentPayload = {
        agentId,
        prompt: prompt.trim(),
        projectPath: projectPath || undefined,
        baseBranch: baseBranch || undefined,
        planMode: false,
        thinking: true,
        useWorktree,
        permissionMode,
        execModel: selectedModel,
        claudeRuntime: agentId === 'claude-code' ? claudeRuntime : undefined,
        multiCreate: false,
        suppressNotifications: false,
      };

      const result = await tauriInvoke<CreateAgentResult>('create_agent_session', { payload });

      // Start the task
      await tauriInvoke('start_task', { taskId: result.task_id });

      // Select the new task and close the form
      selectTask(result.task_id);
      setCreatingTask(false);
    } catch (err) {
      console.error('Failed to create task:', err);
    } finally {
      setSubmitting(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      handleSubmit();
    }
  };

  return (
    <div className="border-b border-border-subtle bg-bg-surface p-3">
      {/* Close button */}
      <div className="mb-2 flex items-center justify-between">
        <span className="text-xs font-medium text-text-secondary">New Task</span>
        <button
          onClick={() => setCreatingTask(false)}
          className="rounded p-0.5 text-text-tertiary hover:bg-bg-surface-hover hover:text-text-primary"
        >
          <X size={14} />
        </button>
      </div>

      {/* Agent selector -- horizontal pills */}
      <div className="mb-3 flex flex-wrap gap-1">
        {AGENT_IDS.map((id) => {
          const agent = AGENTS[id];
          return (
            <button
              key={id}
              onClick={() => setAgentId(id)}
              className={clsx(
                'rounded-full px-2.5 py-1 text-[11px] font-medium transition-colors',
                agentId === id
                  ? 'text-white'
                  : 'bg-bg-surface-hover text-text-secondary hover:text-text-primary'
              )}
              style={agentId === id ? { backgroundColor: agent.color } : undefined}
            >
              {agent.name}
            </button>
          );
        })}
      </div>

      {/* Prompt textarea */}
      <textarea
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder="Describe what you want the agent to do..."
        rows={3}
        className="mb-2 w-full resize-none rounded-lg border border-border-default bg-bg-input px-3 py-2 text-sm text-text-primary placeholder:text-text-tertiary focus:outline-none focus:ring-2 focus:ring-accent-primary"
        autoFocus
      />

      {/* Project path */}
      <input
        value={projectPath}
        onChange={(e) => setProjectPath(e.target.value)}
        placeholder="Project path (optional)"
        className="mb-2 w-full rounded-md border border-border-default bg-bg-input px-3 py-1.5 text-xs text-text-primary placeholder:text-text-tertiary focus:outline-none focus:ring-1 focus:ring-accent-primary"
      />

      {/* Advanced toggle */}
      <button
        onClick={() => setShowAdvanced(!showAdvanced)}
        className="mb-2 flex items-center gap-1 text-[11px] text-text-tertiary hover:text-text-secondary"
      >
        {showAdvanced ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
        Advanced options
      </button>

      {showAdvanced && (
        <div className="mb-3 space-y-2 rounded-lg bg-bg-base p-2.5">
          {/* Model selector */}
          <div>
            <label className="mb-1 block text-[11px] font-medium text-text-tertiary">Model</label>
            <select
              value={selectedModel}
              onChange={(e) => setSelectedModel(e.target.value)}
              className="w-full rounded-md border border-border-default bg-bg-input px-2 py-1.5 text-xs text-text-primary"
            >
              {models.map((m) => (
                <option key={m.value} value={m.value}>
                  {m.name || m.value}{m.isDefault ? ' (default)' : ''}
                </option>
              ))}
            </select>
          </div>

          {/* Permission mode */}
          <div>
            <label className="mb-1 block text-[11px] font-medium text-text-tertiary">Permissions</label>
            <select
              value={permissionMode}
              onChange={(e) => setPermissionMode(e.target.value)}
              className="w-full rounded-md border border-border-default bg-bg-input px-2 py-1.5 text-xs text-text-primary"
            >
              <option value="bypassPermissions">Auto-approve all</option>
              <option value="default">Ask for permission</option>
            </select>
          </div>

          {/* Worktree toggle */}
          <label className="flex items-center gap-2 text-xs text-text-primary">
            <input
              type="checkbox"
              checked={useWorktree}
              onChange={(e) => setUseWorktree(e.target.checked)}
              className="rounded"
            />
            Use git worktree (isolated branch)
          </label>

          {/* Base branch */}
          {useWorktree && (
            <input
              value={baseBranch}
              onChange={(e) => setBaseBranch(e.target.value)}
              placeholder="Base branch (e.g., main)"
              className="w-full rounded-md border border-border-default bg-bg-input px-2 py-1.5 text-xs text-text-primary placeholder:text-text-tertiary"
            />
          )}

          {/* Claude runtime */}
          {agentId === 'claude-code' && (
            <div>
              <label className="mb-1 block text-[11px] font-medium text-text-tertiary">Runtime</label>
              <select
                value={claudeRuntime}
                onChange={(e) => setClaudeRuntime(e.target.value)}
                className="w-full rounded-md border border-border-default bg-bg-input px-2 py-1.5 text-xs text-text-primary"
              >
                <option value="native">Native</option>
                <option value="docker">Docker</option>
              </select>
            </div>
          )}
        </div>
      )}

      {/* Submit */}
      <button
        onClick={handleSubmit}
        disabled={!prompt.trim() || submitting}
        className="flex w-full items-center justify-center gap-2 rounded-lg bg-accent-primary py-2 text-sm font-medium text-white transition-colors hover:bg-accent-primary-hover disabled:opacity-50"
      >
        {submitting ? (
          <>
            <Loader2 size={14} className="animate-spin" />
            Creating...
          </>
        ) : (
          'Create Task'
        )}
      </button>
    </div>
  );
}

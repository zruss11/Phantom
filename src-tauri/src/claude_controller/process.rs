use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::Read;

#[derive(Debug, Clone)]
pub struct SpawnAgentOptions {
    pub team_name: String,
    pub agent_name: String,
    pub agent_id: String, // name@team
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub cwd: String,
    pub parent_session_id: Option<String>,
    pub color: Option<String>,
    pub claude_binary: String,
    pub permission_mode: Option<String>,
    pub allowed_tools: Vec<String>,
    pub env: Vec<(String, String)>,
}

struct AgentProc {
    #[allow(dead_code)]
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
}

#[derive(Clone, Default)]
pub struct ProcessManager {
    inner: Arc<StdMutex<HashMap<String, AgentProc>>>,
}

impl ProcessManager {
    pub fn is_running(&self, agent_name: &str) -> bool {
        let map = self.inner.lock().unwrap();
        map.contains_key(agent_name)
    }

    pub fn spawn(&self, opts: SpawnAgentOptions) -> Result<i32, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("openpty: {e}"))?;

        let mut args: Vec<String> = vec![
            "--teammate-mode".to_string(),
            "auto".to_string(),
            "--agent-id".to_string(),
            opts.agent_id.clone(),
            "--agent-name".to_string(),
            opts.agent_name.clone(),
            "--team-name".to_string(),
            opts.team_name.clone(),
        ];
        if let Some(v) = opts.agent_type.as_ref().filter(|s| !s.trim().is_empty()) {
            args.push("--agent-type".to_string());
            args.push(v.to_string());
        }
        if let Some(v) = opts.color.as_ref().filter(|s| !s.trim().is_empty()) {
            args.push("--agent-color".to_string());
            args.push(v.to_string());
        }
        if let Some(v) = opts
            .parent_session_id
            .as_ref()
            .filter(|s| !s.trim().is_empty())
        {
            args.push("--parent-session-id".to_string());
            args.push(v.to_string());
        }
        if let Some(v) = opts.model.as_ref().filter(|s| !s.trim().is_empty()) {
            args.push("--model".to_string());
            args.push(v.to_string());
        }
        if let Some(v) = opts
            .permission_mode
            .as_ref()
            .filter(|s| !s.trim().is_empty())
        {
            args.push("--permission-mode".to_string());
            args.push(v.to_string());
        }
        for tool in &opts.allowed_tools {
            args.push("--allowedTools".to_string());
            args.push(tool.to_string());
        }

        let mut cmd = CommandBuilder::new(&opts.claude_binary);
        for a in &args {
            cmd.arg(a);
        }
        cmd.cwd(&opts.cwd);

        // Must enable teammate teams.
        cmd.env("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS", "1");
        for (k, v) in &opts.env {
            cmd.env(k, v);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn_command: {e}"))?;
        drop(pair.slave);

        let pid = child.process_id().unwrap_or(0) as i32;

        // Drain PTY output so the child can't block on a full buffer.
        {
            let mut reader = pair
                .master
                .try_clone_reader()
                .map_err(|e| format!("clone_reader: {e}"))?;
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            });
        }

        let proc = AgentProc {
            master: pair.master,
            child,
        };

        let mut map = self.inner.lock().unwrap();
        map.insert(opts.agent_name, proc);
        Ok(pid)
    }

    pub fn kill(&self, agent_name: &str) {
        let mut map = self.inner.lock().unwrap();
        if let Some(mut proc) = map.remove(agent_name) {
            let _ = proc.child.kill();
        }
    }

    pub fn kill_all(&self) {
        let names: Vec<String> = {
            let map = self.inner.lock().unwrap();
            map.keys().cloned().collect()
        };
        for name in names {
            self.kill(&name);
        }
    }
}

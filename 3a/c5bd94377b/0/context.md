# Session Context

## User Prompts

### Prompt 1

start the app in dev

### Prompt 2

<task-notification>
<task-id>b7e17b4</task-id>
<output-file>REDACTED.output</output-file>
<status>completed</status>
<summary>Background command "Start Tauri app in development mode" completed (exit code 0)</summary>
</task-notification>
Read the output file to retrieve the result: REDACTED.output

### Prompt 3

start a new ci build release. do it as 1.0.4.alpha1.

### Prompt 4

Base directory for this skill: /Users/zachrussell/.claude/skills/tauri-release

# Tauri Release

Bump versions and trigger CI releases for Tauri desktop applications.

## When to Use

- User requests a new release or version bump
- User says "release", "push a release", "new version", "bump to X.Y.Z"
- User wants to trigger a CI build for distribution

## Version File Locations

Tauri projects have version information in multiple files that must stay synchronized:

| File | Field | Purpose |
|--...


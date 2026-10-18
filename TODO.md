# TODO

## PR / Issue Integration

- [ ] Worktree creation: Spawn a new worktree from a specified PR or GitHub issue
- [ ] Review status: Display comments left on PR
- [ ] CI Integration: Display status of CI jobs, integrate re-running
- [ ] Merge status: Show progress in merging and conflicts
- [ ] Integration test: Allow live testing across two separate worktrees to find conflicts

## User Workflow

- [ ] Editor / terminal opening: Open a worktree directly in the user's preferred development environment
- [ ] Life-cycle hooks: Setup, validation, and cleanup; blocking prerequisites and background tasks; logs, dry runs, and approvals when project commands change
- [ ] Worktree creation: Prepare dependencies, environment variables, and setup commands for new worktrees
- [ ] Worktree resources: track services as worktree resources (process, port, URL, logs, health)

## Agentic Development

- [ ] Skill: Create a skill for agents to reference to use `jungle`
- [ ] Hand-off: Let agents record completed work, validation results, remaining steps, and current commit; next agent gets a compact
- [ ] Checkpoints: Let agents checkpoint tracked changes and explicitly select untracked files, try an approach, restore it


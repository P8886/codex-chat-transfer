# Commit Workflow

- Complete and verify each user request, then immediately commit its scoped changes.
- Use one commit per request. Keep unrelated requests out of the same commit.
- Use the actual completion time for author and committer timestamps.
- Never invent historical timestamps. When reconstructing old changes, record the timestamp evidence and reconstruction limits in the commit message.
- Do not push or publish releases unless the user requests it.
- Do not commit chat exports, credentials, runtime data, caches, or compiled application packages.

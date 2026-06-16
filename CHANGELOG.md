# Changelog

## [0.2.0] - 2026-06-16

### Changes

- Redesigned the IPC protocol (epic 1-5): new command and response structure replaces the
  previous wire format, affecting all CLI-to-daemon communication.
- Updated all documentation and JSON socket interface examples to reflect the new protocol.
- Added chord support with a bundled chord example project.
- Removed and renamed internal identifiers that referred to "bar", aligning terminology
  across the codebase.
- Added release automation via a `/release` skill that bumps the version and writes the
  changelog in one step.
- Added Allium specification and project roadmap to support ongoing architecture planning.

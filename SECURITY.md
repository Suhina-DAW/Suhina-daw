# Security Policy

Suhina parses project files, audio files and plug-in state, hosts third-party plug-ins and
will expose CLI and MCP automation. Treat files, paths, plug-in data, automation requests and
command parameters as potentially malicious.

## Reporting a vulnerability

Do not open a public issue containing a working exploit, a weaponized file or secrets. Open an
issue with a minimal, non-exploitable summary and ask the maintainers for a private channel, or
use GitHub's private vulnerability reporting if it is enabled for the repository.

A useful report includes the affected version or commit, operating system and architecture,
the affected component, exact reproduction steps, expected and actual behaviour, and any
crash log or backtrace (with personal paths and data removed).

## Scope

- Project, clip, preset and audio file parsing (decompression bombs, oversized sizes, integer
  overflow, recursion, hangs).
- Plug-in hosting and plug-in state handling.
- Import/export path handling (traversal, symlinks, unsafe overwrites).
- The CLI, MCP server and any control channel.
- Dependencies, CI, packaging and release artifact integrity.

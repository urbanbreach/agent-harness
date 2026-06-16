## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.

## 2025-02-14 - Embedded Shell Option Path Escape Bypass
**Learning:** Paths embedded tightly into shell options (like `-I/etc/passwd` or `--include=/foo`) can bypass workspace boundary checks if path extraction relies only on finding an equals sign (`split_once('=')`) or checking if the entire string `starts_with('/')`.
**Action:** Always safely advance character iterators (using `chars().next()`) past flag prefixes to extract inner path payloads for robust workspace validation, and ensure tests account for payloads that don't use space or '=' separation.

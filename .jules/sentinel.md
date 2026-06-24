## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.

## 2024-05-18 - [Path validation bypass using embedded short flags]
**Learning:** When validating bash commands, paths directly attached to short flags (e.g., `-I/etc/passwd`) without an `=` sign easily bypassed earlier split logic that only looked for `-flag=value` formats. Blindly treating the token as safe if `split_once('=')` failed resulted in workspace escapes. Stripping leading alphanumeric characters using methods like `trim_start_matches` illegally mutated actual paths if they started with a letter.
**Action:** Always extract embedded short flag paths by safely advancing the token iterator past the `-` and the single flag character (using `chars().next()` twice). If the resulting remainder begins with a path character (`/` or `.`), explicitly yield it to the standard workspace validation engine.

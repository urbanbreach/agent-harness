## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.
## 2026-06-14 - [Short Shell Option Path Escapes]
**Learning:** When parsing paths embedded directly in short shell options (e.g., `-I/etc/passwd`), relying on standard `split_once('=')` leaves short options unchecked, and using stripping logic like `trim_start_matches(|c| c.is_alphanumeric())` illegally mutates valid file paths.
**Action:** Always safely advance past the `-` and the single flag character (using `chars().next()` twice) to extract and validate the correct path payload inside short shell options.

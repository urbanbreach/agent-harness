## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.

## 2024-06-06 - Embedded paths in short shell options bypass workspace boundary validation
**Learning:** Shell argument validation failed to extract paths embedded directly within short options (e.g. `-I/etc/passwd`). Because there was no `=` sign and it wasn't stripped correctly without breaking valid paths, it skipped validation entirely, allowing workspace sandbox escapes.
**Action:** Safely parse short options by skipping the `-` and the single flag character (e.g., using `chars().next()` twice) to extract the embedded value, ensuring it undergoes the same workspace boundary checks as other path arguments.

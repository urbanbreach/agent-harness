## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.
## 2024-06-19 - [Embedded path extraction trap in short flags]
**Learning:** Checking for `=` to extract path flags completely misses path escapes embedded directly in short flags (e.g. `-I/etc/passwd`). These are syntactically valid in `ls`, `grep`, etc., and easily bypass strict boundary matching if ignored.
**Action:** Always parse single-character short flags without `=` by safely stepping past the flag dash and character (e.g. `chars().next()` twice) rather than ignoring them or using string trimming functions that consume valid path characters.

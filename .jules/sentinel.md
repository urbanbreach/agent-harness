## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.
## 2024-06-20 - [Short Shell Option Path Traversal]
**Learning:** When parsing paths embedded directly in short shell options (e.g., `-I/etc/passwd`), relying solely on `split_once('=')` leaves them unchecked, as they will be treated as flags and their payloads won't be validated against workspace boundaries.
**Action:** When extracting arguments from short shell flags, safely advance past the hyphen and flag character (e.g. `chars().next()` twice) rather than splitting on `=`. Check `is_empty()` after extraction to ignore flags without payloads.

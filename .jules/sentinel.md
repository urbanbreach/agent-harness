## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.

## 2024-05-20 - Options concatenated with short flags bypass workspace boundary validation
**Learning:** Checking for slashes in arguments and validating paths in long options (`--flag=/etc/passwd`) is insufficient because paths can also be concatenated directly to short flags (`-I/etc/passwd`). Relying on `split_once('=')` blindly skips these inputs entirely, allowing severe relative and absolute path traversals. Blindly removing alphanumeric prefixes using `trim_start_matches` mutates valid paths.
**Action:** When parsing paths in short shell options (e.g., `-I/etc/passwd`), do not rely solely on `split_once('=')`. Instead, check if the argument starts with a single `-` and has more than two characters. Use iterators (`chars().next()` twice) to safely skip the hyphen and the short flag character, extracting the exact concatenated path payload for workspace bounds checking.

## 2024-05-19 - Internal path traversal bypasses workspace boundary validation
**Learning:** Shell argument validation in `looks_like_shell_path_argument` incorrectly ignored tokens with internal relative path elements (e.g. `foo/../../../etc/passwd`) unless they started with specific path prefixes like `./` or `../`. This allowed relative path traversals to bypass workspace path normalization completely.
**Action:** Always validate arguments containing `/` as potential paths to ensure they don't break out of the workspace, rather than only matching strict prefixes.

## 2024-05-19 - Options and Glob strings also bypass workspace boundary validation
**Learning:** Checking for slashes in arguments is a good start, but shell options (`--git-dir=/etc/passwd`) and paths containing glob characters (`foo/../../../etc/pas*`) also need to be extracted and evaluated. The original code skipped anything starting with `-` or containing glob sequences before looking for paths.
**Action:** Extract the value component of options (e.g. splitting on `=`), and extract the static path prefix before glob sequences, so that the base path is always evaluated against the workspace bounds.
## 2024-05-31 - Short options boundary traversal vulnerability bypass
**Learning:** Checking for slashes in arguments works except when the argument is a concatenated short option with no `=` (e.g. `ls -C/tmp` or `ls -f../etc/passwd`). These pass validation when they use slice by bytes instead of characters. Using `token[2..]` in Rust panics on short options passing multi-byte unicode characters, which creates a Denial of Service vulnerability.
**Action:** When validating shell paths in short options, use `chars.as_str()` after consuming the hyphen and the short option letter to safely extract the value while avoiding multi-byte index panics.

# GitHub tools

`github.issue` supports `get`, `list`, `comment`, `close`, and `reopen`.
`github.pull_request` supports `get`, `list`, `comment`, and `create`.
These operations use GitHub's REST [issue](https://docs.github.com/en/rest/issues/issues),
[pull request](https://docs.github.com/en/rest/pulls/pulls), and
[comment](https://docs.github.com/en/rest/issues/comments) endpoints.

Pass `owner` and `repo` together, or omit both to use `HARNESS_GITHUB_REPOSITORY`
then `GITHUB_REPOSITORY`, in `owner/repo` form. Names are validated and normalized
to lowercase. Permission selectors are `owner/repo:operation`, under the tool ID;
for example, `github.issue` with `acme/project:comment`.

`get`, `comment`, `close`, and `reopen` require a positive `issue_number` or
`pull_number`, as appropriate for the tool. Lists and pull request creation do
not require a number. Comments require a nonblank `body`. Creating a pull
request requires nonblank `title`, `head`, and `base`, with optional `body`
and `draft`. `title`, `head`, `base`, and every body must fit within 64 KiB.
Bodies retain newlines.

Lists accept `state` (`open`, `closed`, or `all`, default `open`), `per_page`
(default 20, clamped to 1 through 100), and `page` (default 1; zero becomes 1).
Each call fetches one page. `has_more` reflects the next-page link; increment
`page` to continue. Issue lists exclude pull requests. Their count may therefore
be smaller than `per_page`.

Token precedence is `HARNESS_GITHUB_TOKEN`, `GITHUB_TOKEN`, then `GH_TOKEN`.
Mutations require a token. Public reads can run anonymously. The API base defaults
to `https://api.github.com`; `HARNESS_GITHUB_API_BASE_URL` can select an HTTP(S)
enterprise endpoint, including its path prefix. Embedded URL credentials, query
parameters, and fragments are rejected. These environment settings are captured
when the tools are registered. Tokens enter the runtime redactor before tool
calls. CLI setup uses its injected environment lookup.

Calls share the bounded HTTP connection pool with web fetches. Responses are
limited to 2 MiB after HTTP decompression, with a 30-second deadline. Redirects
and automatic retries are disabled. Cancellation drops the request. A cancelled
or failed mutation may already have reached GitHub; inspect its state before
retrying. API error bodies are omitted from output and history.

Results retain structured issue, pull request, comment, or list data alongside a
text summary. The coordinator redacts credentials and applies normal output and
artifact limits. Registration, replay, and inspection perform no GitHub requests.

A local HTTP fixture covers all operations, request bodies and headers, pagination,
repository validation, permission denial, anonymous reads, authentication failures,
response bounds, redirects, and cancellation:

```bash
cargo nextest run -p harness-tools --test github
```

This fixture does not verify a live account's token scopes or repository access.

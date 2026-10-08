# CI integration

`odl check` exits with `1` when it finds violations, so it fails a CI job on
its own. The output formats below additionally show the findings where you
review code.

Until odoo-lint 0.1.0 is released, `uvx` needs `--prerelease allow` to
install the alpha versions from PyPI.

## GitHub

### The odoo-lint action

[odoo-lint action](https://github.com/marketplace/actions/odoo-lint) on the
GitHub Marketplace installs the wheel and runs `odl check`. It shows the
findings as annotations, or with `sarif: true` sends them to code scanning:

```yaml
- uses: actions/checkout@v6
  with:
    persist-credentials: false
- uses: bosd/odoo-lint-action@v1
  with:
    version: 0.1.0a5 # optional, the latest release otherwise
```

The steps below do the same without the action.

### Annotations on pull requests

The `github` format prints workflow commands that GitHub shows as annotations
on the changed lines:

```yaml
- uses: astral-sh/setup-uv@v8
- run: uvx --prerelease allow --from odoo-linter odl check --output-format github
```

### Code scanning (SARIF)

Code scanning keeps track of alerts across runs: new and fixed findings,
dismissals with a reason, and a link to the rule documentation for every
alert. It is free for public repositories; private repositories need GitHub
Advanced Security.

```yaml
name: odoo-lint

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

jobs:
  odoo-lint:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      security-events: write
    steps:
      - uses: actions/checkout@v6
        with:
          persist-credentials: false
      - uses: astral-sh/setup-uv@v8
      - name: Lint
        run: uvx --prerelease allow --from odoo-linter odl check --output-format sarif > odoo-lint.sarif
        continue-on-error: true
      - name: Upload to code scanning
        if: always()
        uses: github/codeql-action/upload-sarif@v4
        with:
          sarif_file: odoo-lint.sarif
          category: odoo-lint
```

Pin the actions to a full commit SHA in your own workflows.

## GitLab

The `gitlab` format is a
[Code Quality report](https://docs.gitlab.com/ci/testing/code_quality/).
GitLab shows the findings in the merge request widget and in the changes
view:

```yaml
odoo-lint:
  image: ghcr.io/astral-sh/uv:python3.13-bookworm-slim
  script:
    - uvx --prerelease allow --from odoo-linter odl check --output-format gitlab > gl-code-quality-report.json
  artifacts:
    when: always
    reports:
      codequality: gl-code-quality-report.json
```

## Forgejo and Gitea

Forgejo and Gitea Actions do not show annotations or code quality reports.
[reviewdog](https://github.com/reviewdog/reviewdog) can turn the `sarif`
output into review comments on the pull request instead; its
`gitea-pr-review` reporter also works with Forgejo, which speaks the Gitea
API.

```yaml
on: [pull_request]

jobs:
  odoo-lint:
    runs-on: docker
    container:
      image: ghcr.io/astral-sh/uv:python3.13-bookworm
    steps:
      - uses: https://code.forgejo.org/actions/checkout@v4
      - name: Install reviewdog
        run: |
          curl -sSfL -o /tmp/reviewdog.tar.gz \
            https://github.com/reviewdog/reviewdog/releases/download/v0.21.2/reviewdog_0.21.2_Linux_x86_64.tar.gz
          echo "30413aa3c7443e9c3c157fe5766cad40e3bb39a32e210ee69b710a8d5c4b8e51  /tmp/reviewdog.tar.gz" | sha256sum -c -
          tar -xzf /tmp/reviewdog.tar.gz -C /usr/local/bin reviewdog
      - name: Lint and comment
        env:
          REVIEWDOG_GITEA_API_TOKEN: ${{ secrets.REVIEWDOG_TOKEN }}
          GITEA_ADDRESS: ${{ github.server_url }}
        run: |
          uvx --prerelease allow --from odoo-linter odl check --output-format sarif > odoo-lint.sarif || true
          reviewdog -f=sarif -name=odoo-lint -reporter=gitea-pr-review -filter-mode=added < odoo-lint.sarif
```

`REVIEWDOG_TOKEN` is an access token of a user that may comment on pull
requests. The checksum is the one reviewdog publishes for v0.21.2. This recipe follows reviewdog's documentation and has not been
tested by the odoo-lint project on a Forgejo instance; reports are welcome.

## Other tools

The `sarif` format also works with the VS Code SARIF Viewer, Azure DevOps,
SonarQube's external issue import and DefectDojo. The `json` format is a plain
array for your own scripts.

## README badges

`odl badge` writes a [shields.io endpoint](https://shields.io/badges/endpoint-badge)
JSON file:

```bash
odl badge -o odoo-lint.json                       # odoo-lint | 94% clean
odl badge --upgrade 19.0 -o odoo-upgrade.json     # Odoo 19.0 | ready (or "12 changes")
```

The share of clean modules counts the modules without findings, with the
rules your configuration selects, so align `select` and `ignore` with your
policy first. The upgrade badge is green when
[`odl upgrade-check`](upgrades.md) finds nothing.

A workflow can publish both to a `badges` branch on every push:

```yaml
name: badges

on:
  push:
    branches: [main]

permissions: {}

jobs:
  badges:
    runs-on: ubuntu-latest
    permissions:
      contents: write # push to the badges branch
    steps:
      - uses: actions/checkout@v6
      - uses: astral-sh/setup-uv@v8
      - run: |
          mkdir -p /tmp/badges
          uvx --prerelease allow --from odoo-linter odl badge -o /tmp/badges/odoo-lint.json
          uvx --prerelease allow --from odoo-linter odl badge --upgrade 19.0 -o /tmp/badges/odoo-upgrade.json
          git config user.name "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
          git switch --orphan badges
          git rm -rfq --cached . && git clean -fdxq
          cp /tmp/badges/*.json .
          git add *.json && git commit -qm "Update badges"
          git push -f origin badges
```

And in the README:

```markdown
![odoo-lint](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/OWNER/REPO/badges/odoo-lint.json)
![Odoo upgrade](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/OWNER/REPO/badges/odoo-upgrade.json)
```

Pin the actions to a full commit SHA in your own workflows.

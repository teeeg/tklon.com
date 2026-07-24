## What is this

Personal blog at tklon.com. A single Rust binary (`tklon`) generates the site
from Markdown plus committed media. Pushing to master builds it and syncs to
S3/CloudFront.

## Directory structure

```
tklon.com
├── .github/workflows/   --> build + test on PRs, deploy on master
├── Cargo.toml           --> workspace
├── Makefile             --> infra / publish / deploy / stats
├── deploy
│   ├── lambda/          --> stats report Lambda
│   ├── params.yml       --> per-deployment values
│   └── template.yml     --> CloudFormation template
├── generator            --> the tklon SSG
│   └── src/
└── site
    ├── config.yml       --> site identity
    ├── templates/       --> Tera templates
    ├── e2e/             --> link checks
    ├── data/            --> image + video manifests
    ├── images/          --> image masters, gitignored
    ├── videos/          --> video masters, gitignored
    └── source
        ├── posts/{year}/
        ├── pages/       --> standalone pages
        ├── stylesheets/ --> SCSS
        ├── images/      --> responsive variants
        └── fonts/
```

## Local development

Requires [Rust](https://www.rust-lang.org/). The workspace manifest at the repo
root lets cargo run from anywhere in the tree.

```sh
cargo run -- serve      # http://localhost:4567, rebuilds on save
cargo run -- build      # production build into build/
cargo test              # generator unit tests
make e2e                # build, then check internal + external links
```

`cargo test` covers the generator, which knows nothing about this site's
content. The tests that do — every internal link resolves to a file in the
build, every external link is still alive, the tag filter's markup, CSS and
script agree on the same tags — live in [`site/e2e/`](site/e2e/) and run
against a build rather than a running generator. Only an unambiguous
404/410 counts as a dead external link; bot walls, rate limits and timeouts
are reported but never fail the run.

To write a post, drop `site/source/posts/{year}/{slug}.md` with `title`, `date`
and `tags` front matter. Images are native Markdown against a manifest name —
`![alt](image-name "caption")`. Video and embeds use
`{{< video name="…" caption="…" >}}` and `{{< embed url="…" title="…" >}}`.

Media is authored with `tklon images` and `tklon video`; see
[`generator/README.md`](generator/README.md) for both commands and the full
output format.

## Deploying

Content and infrastructure deploy separately.

**Content** — pushing to master runs `.github/workflows/ci.yml`, which
builds the site, runs the generator tests and the link checks, then syncs to
S3 and invalidates CloudFront. All three must pass before the deploy runs. A
pull request runs them and stops there. It authenticates to AWS via OIDC, so
there are no stored keys.

**Infrastructure** — applied manually with `make infra`. With the `aws-cli`
installed and authenticated:

```sh
make infra      # deploy/update the CloudFormation stack
make publish    # build + sync content locally, bypassing Actions
make help       # list every target
```

Stack name and region default to `tklondotcom` / `us-east-1`; override inline,
e.g. `make infra STACK=foo REGION=us-west-2`.

Per-deployment values live in [`deploy/params.yml`](deploy/params.yml), which
`make infra` reads and passes to CloudFormation. `template.yml` is the schema
and is not deployable on its own.

- **AcmCertificateArn** ACM certificate covering the apex + www subdomain, in us-east-1
- **Route53HostZoneName** Route 53 hosted zone for the CloudFront alias records
- **GitHubRepositoryOwner** GitHub account or org that owns the repo
- **GitHubRepositoryName** GitHub repository name
- **StatsReportEmail** address for the weekly access-log summary; SNS sends a
  confirmation link on first create/update

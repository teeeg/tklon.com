## What is this

Personal blog at tklon.com. A single Rust binary (`tklon`) generates the site
from Markdown plus committed media. Pushing to master builds it and syncs to
S3/CloudFront.

## Directory structure

```
tklon.com
├── .github/workflows/   --> deploy, new post, media processing
├── Cargo.toml           --> workspace
├── Makefile             --> infra / publish / deploy / stats
├── deploy
│   ├── lambda/          --> stats report Lambda
│   ├── params.yml       --> per-deployment values
│   └── template.yml     --> CloudFormation template
├── generator            --> the tklon SSG
│   ├── src/
│   └── assets/tags.js   --> client-side tag filter
└── site
    ├── config.yml       --> site identity
    ├── templates/       --> Tera templates
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
```

To write a post, drop `site/source/posts/{year}/{slug}.md` with `title`, `date`
and `tags` front matter. Images are native Markdown against a manifest name —
`![alt](image-name "caption")`. Video and embeds use
`{{< video name="…" caption="…" >}}` and `{{< embed url="…" title="…" >}}`.

Media is authored with `tklon images` and `tklon video`; see
[`generator/README.md`](generator/README.md) for both commands and the full
output format.

## Posting from your phone

A photo post needs no laptop — the whole loop is in the GitHub mobile app:

1. **Actions → "new post" → title → Run** creates a dated branch, a stub with
   today's date, and a draft PR.
2. **Edit the post** in the PR: write prose, and drop `![alt text](@)` wherever a
   photo goes — one `@` per photo, in order.
3. **Attach the photos** to the PR description, in that same order.
4. `process-media.yml` fills the `@` slots, encodes variants, backs up the
   masters, commits the result, and keeps a sticky preview comment in sync.
5. Green checks → **merge → live.**

Photos go via attachments only, which GitHub caps at ~10 MB on the free plan.
For video, encode on desktop with `tklon video`. HEIC isn't supported — iOS
normally converts to JPEG on upload, and the run fails loudly if it doesn't.

## Deploying

Content and infrastructure deploy separately.

**Content** — pushing to master runs `.github/workflows/deploy.yml`, which
builds the site, syncs it to S3 and invalidates CloudFront. It authenticates to
AWS via OIDC, so there are no stored keys.

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

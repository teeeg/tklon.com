## What is this

Infrastructure for a CDN-backed S3 bucket that can be used with any static site generator. Pushing to master triggers a GitHub Actions workflow that builds the site and syncs it to S3/Cloudfront.

## Directory structure

```
tklon.com
├── .github/workflows/deploy.yml --> builds + deploys on push to master
├── Makefile --> deploy + local dev shortcuts
├── deploy
│   ├── lambda/ --> stats report Lambda
│   ├── params.yml --> per-deployment values
│   └── template.yml --> CloudFormation template
└── src --> source blog content
```

## Deploying

Content and infrastructure deploy separately:

- **Content** — pushing to master runs `.github/workflows/deploy.yml`, which builds the site and syncs it to S3 + invalidates Cloudfront. The workflow authenticates to AWS via OIDC (no stored keys).
- **Infrastructure** in `template.yml` is applied manually.

With the `aws-cli` installed and authenticated (e.g. `aws login`):  
`make infra` to deploy/update the CloudFormation stack  
`make publish` to build + sync content locally (manual deploy, bypasses Actions)  
`make help` to list every target

Stack name and region default to `tklondotcom` / `us-east-1`; override inline, e.g. `make infra STACK=foo REGION=us-west-2`.

Per-deployment values live in [`deploy/params.yml`](deploy/params.yml) (single source of truth). `make infra` reads it and passes values to CloudFormation; `template.yml` is the schema only and is not standalone-deployable without `--parameter-overrides`. Edit and re-run `make infra` to apply.

- **AcmCertificateArn** ACM certificate [ARN](https://docs.aws.amazon.com/general/latest/gr/aws-arns-and-namespaces.html) covering the apex + www subdomain (must live in us-east-1)
- **Route53HostZoneName** Route 53 hosted zone where alias records to CloudFront are created
- **GitHubRepositoryOwner** GitHub account or org that owns the repo (scopes the OIDC deploy role)
- **GitHubRepositoryName** GitHub repository name (scopes the OIDC deploy role)
- **StatsReportEmail** address that receives the weekly access-log summary; SNS emails a confirmation link on first create/update — click it to start delivery

## Local development

Requirements:
- [Ruby](https://www.ruby-lang.org/) (managed with rbenv via `src/.ruby-version`)
- [Node](https://nodejs.org/)

`make install` to install gems + npm deps  
`make serve` to run the dev server at http://localhost:4567  
`make build` for a production build into `src/build`  
`make test` to run the JS tests

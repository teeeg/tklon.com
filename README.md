## What is this

Infrastructure for a CDN-backed S3 bucket that can be used with any static site generator. Changes merged to master will trigger the build script defined by `buildspec.yml` and sync to S3/Cloudfront.

## Directory structure

```
tklon.com
├── Makefile --> deploy + local dev shortcuts
├── deploy
│   ├── buildspec.yml --> CodeBuild commands to build and sync content
│   └── template.yml --> CloudFormation template (CDN, DNS aliases, CloudFront function, CI pipeline)
└── src --> source content middleman builds into static assets
```

## Deploying

Content and infrastructure deploy separately:

- **Content** merged to master triggers the CodePipeline (`buildspec.yml`), which builds the site and syncs it to S3/Cloudfront.
- **Infrastructure** in `template.yml` is applied manually — the pipeline does not touch it.

With the `aws-cli` installed and authenticated (e.g. `aws login`):  
`make infra` to deploy/update the CloudFormation stack  
`make publish` to build + sync content without the pipeline  
`make deploy` to do infra then content  
`make help` to list every target

Stack name and region default to `tklondotcom` / `us-east-1`; override inline, e.g. `make infra STACK=foo REGION=us-west-2`.

`template.yml` includes a few configurable parameters (all have defaults):

- **AcmCertificateArn** to serve resources over HTTPS, your ACM certificate [ARN](https://docs.aws.amazon.com/general/latest/gr/aws-arns-and-namespaces.html) associated with the Cloudfront distribution (must live in us-east-1)
- **Route53HostZoneName** the top-level domain name of the Route53 hosted zone within which alias records will be created to route traffic to the Cloudfront distribution
- **GitHubRepositoryOwner** the GitHub account or organization that owns the repository, e.g. teeeg
- **GitHubRepositoryName** the GitHub repository name, e.g. tklon.com

## Local development

Requirements:
- [Ruby](https://www.ruby-lang.org/) (managed with rbenv via `src/.ruby-version`)
- [Node](https://nodejs.org/)

`make install` to install gems + npm deps  
`make serve` to run the dev server at http://localhost:4567  
`make build` for a production build into `src/build`  
`make test` to run the JS tests

## What is this

Infrastructure for a CDN-backed S3 bucket that can be used with any static site generator. Changes merged to master will trigger build script defined by `buildspec.yml` and sync to S3/Cloudfront.

## Directory structure

```
tklon.com
├── deploy
│   ├── buildspec.yml --> CodeBuild commands to build and deploy assets
│   └── template.yml --> CloudFormation templates (CDN, DNS aliases, CD pipeline, etc.)
└── src --> Structured data that buildspec will transform into static assets
```

## Deploying CloudFormation changes

`template.yml` includes a few configurable parameters:

- **AcmCertificateArn** to serve resources over HTTPS, your ACM certificate [ARN](https://docs.aws.amazon.com/general/latest/gr/aws-arns-and-namespaces.html) is associated with the Cloudfront distribution
- **Route53HostZoneName** the top-level domain name of the Route53 hosted zone within which alias records will be created to route traffic to the Cloudfront distribution
- **GitHubRepositoryOwner** the GitHub account or organization that owns the repository, e.g. teeeg
- **GitHubRepositoryName** the GitHub repository name, e.g. tklon.com

To create you must have the `aws-cli` installed and configured:  
`aws cloudformation create-stack --stack-name myteststack --template-body file://deploy/template.yml --parameters AcmCertificateArn=arn:aws:acm:us-east-1:295005258746:certificate/2f702844-d6e5-4573-8a41-57fcc0c1992b,Route53HostZoneName=tklon.com,GitHubRepositoryOwner=teeeg,GitHubRepositoryName=tklon.com --capabilities CAPABILITY_AUTO_EXPAND CAPABILITY_IAM`

To update:
`aws cloudformation update-stack --stack-name myteststack --template-body file://deploy/template.yml --capabilities CAPABILITY_AUTO_EXPAND CAPABILITY_IAM`

## Local development

Requirements:
- [Ruby](https://www.ruby-lang.org/)
- [RubyGems](https://rubygems.org/)
- [NPM](https://docs.npmjs.com/) CLI

From `src` dir:  
`gem install bundle`  
`bundle install`  
`npm install`  
`bundle exec middleman`

To test production build: 
`bundle exec middleman build --verbose`
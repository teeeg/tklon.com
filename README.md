
## Directory structure
```
tklon.com
├── deploy
│   ├── buildspec.yaml --> CodeBuild commands to build and deploy assets
│   └── template.yaml --> CloudFormation template defining infrastructure (CDN, DNS aliases, CD pipeline, etc.)  
└── source --> Raw website source content, layouts, JavaScripts, and styles 
    ├── layouts  
    ├── partials  
    ├── posts
    ├── stylesheets
    └── typescripts
```

## Deploying CloudFormation changes
`template.yaml` includes parameter defaults but can be modified for your needs: 
- **AcmCertificateArn** to support HTTPS, an ACM certificate [ARN](https://docs.aws.amazon.com/general/latest/gr/aws-arns-and-namespaces.html) is associated with the Cloudfront distribution 
- **Route53HostZoneName** the top-level domain name of the Route53 hosted zone within which alias records will be created to route traffic to the Cloudfront distribution
- **GitHubRepositoryOwner** the GitHub account or organization that owns the repository, e.g. teeeg
- **GitHubRepositoryName** the GitHub repository name, e.g. tklon.com

To create a stack with the `aws-cli`:  
`aws cloudformation create-stack --stack-name myteststack --template-body file://deploy/template.yaml --parameters AcmCertificateArn=arn:aws:acm:us-east-1:295005258746:certificate/2f702844-d6e5-4573-8a41-57fcc0c1992b,Route53HostZoneName=tklon.com,GitHubRepositoryOwner=teeeg,GitHubRepositoryName=tklon.com`

## Development
`gem install bundle`  
`bundle install`  
`bundle exec middleman server`
 


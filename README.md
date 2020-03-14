
# AWS Cloudformation

`buildspec.yaml` defines how the assets are built and deployed by CodeBuild
`template.yaml` creates all resources (S3 bucket, CloudFront distribution, CodeBuild project). It needs only to be run 
once. `aws cloudformation deploy --template-file /deploy/template.yaml --parameters --stack-name my-blog-name`

# Deploy
`middleman s3_sync --build` 

# Development 
`middleman` will run locally, and watch all files (including typescript files through webpack config)

 # AWS Credentials
 Stored in YAML format within the `.s3_sync` file 

